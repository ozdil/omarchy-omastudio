//! Storage services through rclone, with OmaStudio's own encrypted rclone config.
//!
//! The config is encrypted by rclone itself (`rclone config encryption`); its key is
//! a random value stored in the Secret Service keyring and handed to rclone through
//! `--password-command secret-tool lookup ...`, so neither the key nor the tokens are
//! ever on disk in plain text.
//!
//! Signing in drives rclone's configuration questions non-interactively
//! (`config create --non-interactive`, then `config update --continue`), answering
//! "use the browser" for OAuth and the service's defaults for the rest.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::{ConnectEvent, Entry, MAX_TRANSFER_BYTES, OAuthClient, UPLOAD_FOLDER, config_dir, last_error, run, run_streaming, tool, which};
use crate::security;

// Same scheme as the AI provider keys (service=omastudio provider=<id>)
const KEY_ATTRS: [&str; 4] = ["service", "omastudio", "purpose", "cloud-accounts"];
const LIST_TIMEOUT: Duration = Duration::from_secs(120);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// Time the user has to finish signing in
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const QUICK_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_QUESTIONS: usize = 24;

fn config_path() -> PathBuf {
    config_dir().join("rclone.conf")
}

fn password_command() -> String {
    format!("secret-tool lookup {}", KEY_ATTRS.join(" "))
}

/// `rclone <args> --config ... --password-command ...`
fn rclone(args: &[&str]) -> std::process::Command {
    let mut cmd = tool("rclone");
    cmd.args(args)
        .arg("--config")
        .arg(config_path())
        .arg("--password-command")
        .arg(password_command())
        .arg("--ask-password=false");
    cmd
}

fn require_tools() -> Result<(), String> {
    if !which("rclone") {
        return Err("rclone is not installed (sudo pacman -S rclone)".into());
    }
    if !which("secret-tool") {
        return Err("secret-tool is not installed (sudo pacman -S libsecret)".into());
    }
    Ok(())
}

/// Makes sure the keyring holds the config key and the config file is encrypted with it.
fn prepare_config() -> Result<(), String> {
    require_tools()?;
    let path = config_path();
    security::ensure_secure_dir(&config_dir()).map_err(|e| format!("Cannot create {}: {e}", config_dir().display()))?;
    let mut lookup = tool("secret-tool");
    lookup.arg("lookup").args(KEY_ATTRS);
    let (code, out, _) = run(lookup, QUICK_TIMEOUT)?;
    let has_key = code == 0 && !out.trim().is_empty();
    let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
    let encrypted = existing.contains("RCLONE_ENCRYPT_V0:");
    if encrypted && !has_key {
        let in_use = super::load_accounts()?
            .iter()
            .any(|a| matches!(super::provider(&a.provider).map(|p| p.backend), Some(super::Backend::Rclone(..))));
        if in_use {
            return Err("The keyring no longer holds the key of OmaStudio's cloud accounts. Disconnect them and connect again.".into());
        }
        // Nothing uses the old config and nothing can read it: start a new one
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        std::fs::rename(&path, path.with_extension(format!("conf.unreadable-{stamp}"))).map_err(|e| e.to_string())?;
        existing.clear();
    }
    if !has_key {
        let key = random_key()?;
        let mut store = tool("secret-tool");
        store.arg("store").arg("--label=OmaStudio cloud accounts").args(KEY_ATTRS);
        let (code, _, err) = security::run_bounded_command_with_stdin(store, QUICK_TIMEOUT, Some(key.as_bytes()))
            .map(|(c, o, e)| (c, o, String::from_utf8_lossy(&e).into_owned()))
            .map_err(|e| e.to_string())?;
        if code != 0 {
            return Err(format!(
                "Cannot store a key in the system keyring ({}). Is a keyring such as gnome-keyring running?",
                last_error(&err, "secret-tool failed")
            ));
        }
    }
    if !encrypted {
        // An empty config (or one left unencrypted) is encrypted before any token goes in
        security::atomic_write_secure(&path, existing.as_bytes()).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
        let mut cmd = rclone(&["config", "encryption", "set"]);
        cmd.env("RCLONE_PASSWORD_CHANGE", "1");
        let (code, _, err) = run(cmd, QUICK_TIMEOUT)?;
        if code != 0 {
            return Err(format!("Cannot encrypt the rclone config: {}", last_error(&err, "rclone failed")));
        }
    }
    Ok(())
}

fn random_key() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    // SAFETY: bytes is a valid buffer of its length
    let n = unsafe { libc::getrandom(bytes.as_mut_ptr().cast(), bytes.len(), 0) };
    if n != bytes.len() as isize {
        return Err("No random source".into());
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct Question {
    #[serde(default)]
    state: String,
    #[serde(default)]
    option: Option<Value>,
    #[serde(default)]
    error: String,
}

/// The answer OmaStudio gives to one of rclone's configuration questions.
fn answer(option: &Value) -> Result<String, String> {
    let name = option.get("Name").and_then(Value::as_str).unwrap_or_default();
    match name {
        // Sign in through the browser on this machine
        "config_is_local" => return Ok("true".into()),
        // rclone's shared Google client is being retired; it is used only when no own
        // client ID was given
        "config_shared_client_id" => return Ok("true".into()),
        _ => {}
    }
    let default = option
        .get("ValueStr")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| option.get("DefaultStr").and_then(Value::as_str).filter(|s| !s.is_empty()));
    if let Some(d) = default {
        return Ok(d.to_string());
    }
    let first_example = option
        .get("Examples")
        .and_then(Value::as_array)
        .and_then(|e| e.first())
        .and_then(|e| e.get("Value"))
        .and_then(Value::as_str);
    let required = option.get("Required").and_then(Value::as_bool).unwrap_or(false);
    match first_example {
        Some(v) if required => Ok(v.to_string()),
        _ if !required => Ok(String::new()),
        _ => {
            let help = option.get("Help").and_then(Value::as_str).unwrap_or(name).lines().next().unwrap_or(name);
            Err(format!("The service asks for a setting OmaStudio cannot answer: {help}"))
        }
    }
}

/// The sign-in URL rclone prints when it opens the browser.
fn sign_in_url(line: &str) -> Option<String> {
    let start = line.find("http://127.0.0.1:")?;
    let url: String = line[start..].chars().take_while(|c| !c.is_whitespace()).collect();
    url.contains("/auth").then_some(url)
}

/// Creates remote `id`, signs in and creates the upload folder. Returns the user's
/// e-mail or login when the service reports it.
pub fn connect(
    id: &str,
    kind: &str,
    options: &[(&str, &str)],
    client: &OAuthClient,
    progress: &mut dyn FnMut(ConnectEvent),
) -> Result<String, String> {
    prepare_config()?;
    let result = sign_in(id, kind, options, client, progress);
    if let Err(e) = result {
        let _ = forget(id);
        return Err(e);
    }
    let (code, _, err) = run(rclone(&["mkdir", &format!("{id}:{UPLOAD_FOLDER}")]), LIST_TIMEOUT)?;
    if code != 0 {
        let _ = forget(id);
        return Err(format!("Signed in, but the {UPLOAD_FOLDER} folder could not be created: {}", last_error(&err, "rclone failed")));
    }
    Ok(user_name(id))
}

fn sign_in(
    id: &str,
    kind: &str,
    options: &[(&str, &str)],
    client: &OAuthClient,
    progress: &mut dyn FnMut(ConnectEvent),
) -> Result<(), String> {
    let mut args: Vec<String> = vec!["config".into(), "create".into(), id.into(), kind.into()];
    args.extend(options.iter().map(|(k, v)| format!("{k}={v}")));
    if !client.client_id.is_empty() {
        args.push(format!("client_id={}", client.client_id));
        args.push(format!("client_secret={}", client.client_secret));
    }
    args.push("--non-interactive".into());
    let mut reported = false;
    let mut on_line = |line: &str| {
        if !reported && let Some(url) = sign_in_url(line) {
            reported = true;
            progress(ConnectEvent::Browser { url });
        }
    };
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut step = run_streaming(rclone(&refs), SIGN_IN_TIMEOUT, &mut on_line)?;
    for _ in 0..MAX_QUESTIONS {
        let (code, out, err) = step;
        if code != 0 {
            return Err(format!("Sign-in failed: {}", last_error(&err, "rclone failed")));
        }
        let q: Question = serde_json::from_str(&out).map_err(|_| format!("Sign-in failed: {}", last_error(&err, "unexpected answer from rclone")))?;
        if !q.error.is_empty() {
            return Err(format!("Sign-in failed: {}", q.error));
        }
        if q.state.is_empty() {
            return Ok(());
        }
        let reply = answer(q.option.as_ref().unwrap_or(&Value::Null))?;
        let mut cmd = rclone(&["config", "update", id, "--continue", "--non-interactive", "--state", &q.state, "--result", &reply]);
        if !client.client_secret.is_empty() {
            // Passwords are passed again on every step
            cmd.arg(format!("client_secret={}", client.client_secret));
        }
        step = run_streaming(cmd, SIGN_IN_TIMEOUT, &mut on_line)?;
    }
    Err("Sign-in failed: too many setup questions".into())
}

fn user_name(id: &str) -> String {
    let Ok((0, out, _)) = run(rclone(&["config", "userinfo", &format!("{id}:"), "--json"]), QUICK_TIMEOUT) else {
        return String::new();
    };
    let info: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
    ["Email", "email", "Login", "login", "Username", "Name"]
        .iter()
        .find_map(|k| info.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty()))
        .map(|s| s.chars().take(120).collect())
        .unwrap_or_default()
}

/// Removes remote `id` (and with it its tokens) from the config.
pub fn forget(id: &str) -> Result<(), String> {
    if !config_path().exists() {
        return Ok(());
    }
    let (code, _, err) = run(rclone(&["config", "delete", id]), QUICK_TIMEOUT)?;
    if code != 0 {
        return Err(format!("Cannot remove the account's keys: {}", last_error(&err, "rclone failed")));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Item {
    name: String,
    #[serde(default)]
    size: i64,
    #[serde(default)]
    is_dir: bool,
}

pub fn list(id: &str, path: &str) -> Result<Vec<Entry>, String> {
    let (code, out, err) = run(rclone(&["lsjson", "--no-mimetype", "--no-modtime", &format!("{id}:{path}")]), LIST_TIMEOUT)?;
    if code != 0 {
        return Err(format!("Cannot list the folder: {}", last_error(&err, "rclone failed")));
    }
    let items: Vec<Item> = serde_json::from_str(&out).map_err(|e| format!("Cannot list the folder: {e}"))?;
    Ok(items
        .into_iter()
        .filter(|i| !i.name.contains('/') && !i.name.starts_with('.'))
        .map(|i| Entry {
            path: if path.is_empty() { i.name.clone() } else { format!("{path}/{}", i.name) },
            name: i.name,
            is_dir: i.is_dir,
            size: i.size.max(0) as u64,
        })
        .collect())
}

pub fn fetch(id: &str, path: &str, local: &Path) -> Result<(), String> {
    let remote = format!("{id}:{path}");
    let (code, out, err) = run(rclone(&["lsjson", "--stat", "--no-mimetype", &remote]), LIST_TIMEOUT)?;
    if code != 0 {
        return Err(format!("Cannot open the file: {}", last_error(&err, "rclone failed")));
    }
    let item: Item = serde_json::from_str(&out).map_err(|e| format!("Cannot open the file: {e}"))?;
    if item.is_dir {
        return Err(format!("{path} is a folder"));
    }
    let size = item.size.max(0) as u64;
    if size > MAX_TRANSFER_BYTES {
        return Err(format!("{path} is larger than {} GB", MAX_TRANSFER_BYTES >> 30));
    }
    if std::fs::symlink_metadata(local).is_ok_and(|m| m.is_file() && m.len() == size) {
        return Ok(());
    }
    let (code, _, err) = run(rclone(&["copyto", &remote, &local.to_string_lossy()]), TRANSFER_TIMEOUT)?;
    if code != 0 {
        return Err(format!("Download failed: {}", last_error(&err, "rclone failed")));
    }
    security::verify_safe_file(local).map_err(|e| format!("Download failed: {e}"))
}

pub fn upload(id: &str, file: &Path, target: &str) -> Result<(), String> {
    let (code, _, err) = run(rclone(&["copyto", &file.to_string_lossy(), &format!("{id}:{target}")]), TRANSFER_TIMEOUT)?;
    if code != 0 {
        return Err(format!("Upload failed: {}", last_error(&err, "rclone failed")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_rclone_questions() {
        let q = |v: Value| answer(&v);
        assert_eq!(q(serde_json::json!({"Name": "config_is_local", "DefaultStr": "true"})).unwrap(), "true");
        assert_eq!(q(serde_json::json!({"Name": "config_shared_client_id", "DefaultStr": "false"})).unwrap(), "true");
        assert_eq!(q(serde_json::json!({"Name": "config_type", "DefaultStr": "onedrive", "Required": true})).unwrap(), "onedrive");
        assert_eq!(q(serde_json::json!({"Name": "config_driveid", "DefaultStr": "", "Required": true,
            "Examples": [{"Value": "b!abc", "Help": "OneDrive (personal)"}]})).unwrap(), "b!abc");
        assert_eq!(q(serde_json::json!({"Name": "root_folder_id", "DefaultStr": "", "Required": false})).unwrap(), "");
        assert!(q(serde_json::json!({"Name": "client_id", "DefaultStr": "", "Required": true, "Help": "Client Id"})).is_err());
    }

    #[test]
    fn finds_the_sign_in_url() {
        let line = "2026/10/23 NOTICE: If your browser doesn't open automatically go to the following link: http://127.0.0.1:53682/auth?state=xyz";
        assert_eq!(sign_in_url(line).as_deref(), Some("http://127.0.0.1:53682/auth?state=xyz"));
        assert_eq!(sign_in_url("NOTICE: Log in and authorize rclone for access"), None);
    }

    #[test]
    fn random_keys_differ() {
        let a = random_key().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, random_key().unwrap());
    }
}
