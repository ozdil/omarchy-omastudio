//! Cloud accounts: browse, open and upload photos in Google Drive, OneDrive, Dropbox,
//! Box, pCloud, Yandex Disk and GitHub.
//!
//! The storage services go through rclone, which carries the sign-in (OAuth in the
//! system browser) and the API of each service. OmaStudio keeps its own rclone config
//! in `~/.config/omastudio/cloud/rclone.conf`, encrypted with a random key that lives
//! in the system keyring (Secret Service, read with `secret-tool`); the access tokens
//! are never written in plain text. GitHub goes through the GitHub CLI (`gh`), which
//! signs in with GitHub's device flow and keeps its token in the keyring itself.
//!
//! On connecting, a folder named `OmaStudio` is created in the account (on GitHub a
//! private repository of that name) as the default place for uploads.
//!
//! Photos are downloaded on demand into `~/.cache/omastudio/cloud/<account>/...` and
//! opened from there. `accounts.json` only lists the accounts; it holds no secrets.

mod github;
mod rclone;

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::security;

/// The folder (on GitHub: the repository) created in every account for uploads.
pub const UPLOAD_FOLDER: &str = "OmaStudio";

/// Largest photo downloaded from or uploaded to an account.
pub const MAX_TRANSFER_BYTES: u64 = 2 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// An rclone backend type, and options set when the account is created.
    Rclone(&'static str, &'static [(&'static str, &'static str)]),
    Github,
}

#[derive(Clone, Copy, Debug)]
pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub backend: Backend,
}

pub const PROVIDERS: &[Provider] = &[
    Provider { id: "google-drive", label: "Google Drive", backend: Backend::Rclone("drive", &[("scope", "drive")]) },
    Provider { id: "onedrive", label: "OneDrive", backend: Backend::Rclone("onedrive", &[]) },
    Provider { id: "dropbox", label: "Dropbox", backend: Backend::Rclone("dropbox", &[]) },
    Provider { id: "box", label: "Box", backend: Backend::Rclone("box", &[]) },
    Provider { id: "pcloud", label: "pCloud", backend: Backend::Rclone("pcloud", &[]) },
    Provider { id: "yandex-disk", label: "Yandex Disk", backend: Backend::Rclone("yandex", &[]) },
    Provider { id: "github", label: "GitHub", backend: Backend::Github },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Account {
    /// Also the rclone remote name: `<provider>-<n>`
    pub id: String,
    pub provider: String,
    pub label: String,
    /// The signed-in user (e-mail or login) when the service reports it
    #[serde(default)]
    pub user: String,
}

#[derive(Serialize)]
pub struct ProviderStatus {
    pub id: &'static str,
    pub label: &'static str,
    /// The program this service needs, when it is not installed
    pub missing: Option<&'static str>,
    /// Arch package providing it
    pub package: Option<&'static str>,
}

#[derive(Serialize)]
pub struct Status {
    pub providers: Vec<ProviderStatus>,
    pub accounts: Vec<Account>,
    pub upload_folder: &'static str,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    /// Path inside the account, '/'-separated, without a leading '/'
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

/// Progress of `connect`, printed as JSON lines before the result.
#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ConnectEvent {
    /// The sign-in page was opened in the browser (`url` opens it again).
    Browser { url: String },
    /// GitHub's device flow: the code to type on `url`.
    DeviceCode { code: String, url: String },
}

/// Optional OAuth client for services whose shared rclone client is limited (Google).
#[derive(Default, Clone, Debug)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: String,
}

pub fn status() -> Status {
    let rclone = which("rclone");
    let secret = which("secret-tool");
    let gh = which("gh");
    let providers = PROVIDERS
        .iter()
        .map(|p| {
            let (missing, package) = match p.backend {
                Backend::Rclone(..) if !rclone => (Some("rclone"), Some("rclone")),
                Backend::Rclone(..) if !secret => (Some("secret-tool"), Some("libsecret")),
                Backend::Github if !gh => (Some("gh"), Some("github-cli")),
                _ => (None, None),
            };
            ProviderStatus { id: p.id, label: p.label, missing, package }
        })
        .collect();
    Status { providers, accounts: load_accounts().unwrap_or_default(), upload_folder: UPLOAD_FOLDER }
}

/// Signs in to a new account and creates its upload folder. `progress` receives the
/// sign-in steps as they happen.
pub fn connect(provider_id: &str, client: &OAuthClient, progress: &mut dyn FnMut(ConnectEvent)) -> Result<Account, String> {
    let p = provider(provider_id).ok_or_else(|| format!("Unknown service: {provider_id}"))?;
    let mut accounts = load_accounts()?;
    let account = match p.backend {
        Backend::Rclone(kind, options) => {
            let id = next_id(&accounts, p.id);
            let user = rclone::connect(&id, kind, options, client, progress)?;
            Account { id, provider: p.id.into(), label: p.label.into(), user }
        }
        Backend::Github => {
            let user = github::connect(progress)?;
            // One GitHub sign-in per machine (the GitHub CLI's); connecting again refreshes it
            if let Some(existing) = accounts.iter().find(|a| a.provider == p.id && a.user == user) {
                return Ok(existing.clone());
            }
            Account { id: next_id(&accounts, p.id), provider: p.id.into(), label: p.label.into(), user }
        }
    };
    accounts.push(account.clone());
    save_accounts(&accounts)?;
    Ok(account)
}

/// Forgets an account: removes its keys from OmaStudio's rclone config and its
/// downloaded photos from the cache. Nothing in the account itself is touched; a
/// GitHub sign-in belongs to the GitHub CLI and stays.
pub fn disconnect(account_id: &str) -> Result<(), String> {
    let mut accounts = load_accounts()?;
    let account = find(&accounts, account_id)?.clone();
    if let Some(Backend::Rclone(..)) = provider(&account.provider).map(|p| p.backend) {
        // Without rclone or the keyring key the tokens stay encrypted and unusable; the
        // account is forgotten either way
        if let Err(e) = rclone::forget(&account.id) {
            eprintln!("omastudio-engine: {e}");
        }
    }
    accounts.retain(|a| a.id != account.id);
    save_accounts(&accounts)?;
    let cache = cache_dir().join(&account.id);
    if cache.starts_with(cache_dir()) && account.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        let _ = std::fs::remove_dir_all(cache);
    }
    Ok(())
}

/// Folders and files at `path` ("" is the top of the account; on GitHub, the list of
/// repositories). Folders come first.
pub fn list(account_id: &str, path: &str) -> Result<Vec<Entry>, String> {
    let account = account(account_id)?;
    let path = clean_path(path)?;
    let mut entries = match backend(&account)? {
        Backend::Rclone(..) => rclone::list(&account.id, &path)?,
        Backend::Github => github::list(&path)?,
    };
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

/// Downloads a photo into the cache (once; a cached copy of the same size is reused)
/// and returns the local path.
pub fn fetch(account_id: &str, path: &str) -> Result<PathBuf, String> {
    let account = account(account_id)?;
    let path = clean_path(path)?;
    if path.is_empty() {
        return Err("No file given".into());
    }
    let local = cached_path(&account.id, &path)?;
    let parent = local.parent().ok_or("Invalid cache path")?;
    security::ensure_secure_dir(parent).map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
    match backend(&account)? {
        Backend::Rclone(..) => rclone::fetch(&account.id, &path, &local)?,
        Backend::Github => github::fetch(&path, &local)?,
    }
    Ok(local)
}

/// Uploads a local file into `dir` of the account (default: the OmaStudio folder) and
/// returns the path it got there.
pub fn upload(account_id: &str, file: &Path, dir: Option<&str>) -> Result<String, String> {
    let account = account(account_id)?;
    security::verify_safe_file(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let size = std::fs::metadata(file).map_err(|e| e.to_string())?.len();
    if size > MAX_TRANSFER_BYTES {
        return Err(format!("{} is larger than {} GB", file.display(), MAX_TRANSFER_BYTES >> 30));
    }
    let name = file.file_name().and_then(|n| n.to_str()).ok_or("Invalid file name")?;
    let backend = backend(&account)?;
    let dir = match dir {
        Some(d) => clean_path(d)?,
        None if backend == Backend::Github => format!("{}/{UPLOAD_FOLDER}", account.user),
        None => UPLOAD_FOLDER.to_string(),
    };
    let target = clean_path(&format!("{dir}/{name}"))?;
    match backend {
        Backend::Rclone(..) => rclone::upload(&account.id, file, &target)?,
        Backend::Github => github::upload(file, &target)?,
    }
    Ok(target)
}

fn account(id: &str) -> Result<Account, String> {
    let accounts = load_accounts()?;
    Ok(find(&accounts, id)?.clone())
}

fn find<'a>(accounts: &'a [Account], id: &str) -> Result<&'a Account, String> {
    accounts.iter().find(|a| a.id == id).ok_or_else(|| format!("No connected account named {id}"))
}

fn backend(account: &Account) -> Result<Backend, String> {
    provider(&account.provider).map(|p| p.backend).ok_or_else(|| format!("Unknown service: {}", account.provider))
}

fn next_id(accounts: &[Account], provider: &str) -> String {
    (1..).map(|n| format!("{provider}-{n}")).find(|id| accounts.iter().all(|a| &a.id != id)).expect("a free id")
}

/// A path inside an account: '/'-separated, no empty, "." or ".." parts, no control
/// characters. Leading and trailing '/' are dropped.
pub fn clean_path(path: &str) -> Result<String, String> {
    if path.len() > 4096 {
        return Err("Path too long".into());
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        if part.is_empty() {
            continue;
        }
        if part == "." || part == ".." || part.chars().any(char::is_control) || part.len() > 255 {
            return Err(format!("Invalid path: {path}"));
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

/// Where a downloaded photo is kept.
pub fn cached_path(account_id: &str, path: &str) -> Result<PathBuf, String> {
    let path = clean_path(path)?;
    let local = cache_dir().join(account_id).join(&path);
    if !local.starts_with(cache_dir()) || path.is_empty() {
        return Err(format!("Invalid path: {path}"));
    }
    Ok(local)
}

fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(fallback))
}

pub fn config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("omastudio/cloud")
}

pub fn cache_dir() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache").join("omastudio/cloud")
}

fn accounts_path() -> PathBuf {
    config_dir().join("accounts.json")
}

pub fn load_accounts() -> Result<Vec<Account>, String> {
    let path = accounts_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = security::read_secure_file(&path, 1 << 20).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{} is damaged: {e}", path.display()))
}

fn save_accounts(accounts: &[Account]) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(accounts).map_err(|e| e.to_string())?;
    security::atomic_write_secure(&accounts_path(), &json).map_err(|e| format!("Cannot save the account list: {e}"))
}

pub(crate) fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|d| d.join(program).is_file()))
        .unwrap_or(false)
}

/// A helper program run with the engine's cleared environment, plus what sign-in
/// needs: the session bus (keyring) and the display (browser). It dies with the
/// engine, so cancelling a sign-in in the interface also stops its local web server.
pub(crate) fn tool(program: &str) -> Command {
    let mut cmd = session_command(program);
    // SAFETY: prctl is async-signal-safe
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut cmd, || {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    cmd
}

fn session_command(program: &str) -> Command {
    let mut cmd = security::secure_command(program);
    for var in ["DBUS_SESSION_BUS_ADDRESS", "WAYLAND_DISPLAY", "DISPLAY", "BROWSER", "XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE"] {
        if let Some(v) = std::env::var_os(var) {
            cmd.env(var, v);
        }
    }
    cmd.stdin(Stdio::null());
    cmd
}

/// Runs a helper with a deadline. Returns (exit code, stdout, stderr).
pub(crate) fn run(cmd: Command, timeout: Duration) -> Result<(i32, String, String), String> {
    let (code, out, err) = security::run_bounded_command(cmd, timeout).map_err(|e| e.to_string())?;
    Ok((code, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned()))
}

/// Runs a helper whose stderr is read line by line while it works (sign-in steps).
pub(crate) fn run_streaming(
    mut cmd: Command,
    timeout: Duration,
    on_line: &mut dyn FnMut(&str),
) -> Result<(i32, String, String), String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let pid = child.id() as i32;
    let done = Arc::new(AtomicBool::new(false));
    let watchdog = {
        let done = done.clone();
        std::thread::spawn(move || {
            let step = Duration::from_millis(200);
            let mut waited = Duration::ZERO;
            while !done.load(Ordering::Relaxed) {
                if waited >= timeout {
                    // SAFETY: the child leads its own process group (secure_command)
                    unsafe { libc::kill(-pid, libc::SIGKILL) };
                    return;
                }
                std::thread::sleep(step);
                waited += step;
            }
        })
    };
    let stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.take(security::MAX_COMMAND_OUTPUT_BYTES as u64).read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    });
    let mut err = String::new();
    let mut lines = BufReader::new(child.stderr.take().expect("piped stderr")).take(security::MAX_COMMAND_OUTPUT_BYTES as u64);
    let mut line = String::new();
    while lines.read_line(&mut line).unwrap_or(0) > 0 {
        on_line(line.trim_end());
        err.push_str(&line);
        line.clear();
    }
    let status = child.wait().map_err(|e| e.to_string());
    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    let out = reader.join().unwrap_or_default();
    let code = status?.code().unwrap_or(-1);
    Ok((code, out, err))
}

/// The last meaningful line of a helper's error output, for messages.
pub(crate) fn last_error(stderr: &str, fallback: &str) -> String {
    stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(300).collect())
        .unwrap_or_else(|| fallback.to_string())
}

/// Opens a URL in the user's browser (detached; errors are ignored, the interface
/// shows the URL as well).
pub(crate) fn open_browser(url: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:")) {
        return;
    }
    // The browser must outlive the engine: no death signal for it
    let mut cmd = session_command("xdg-open");
    cmd.arg(url).stdout(Stdio::null()).stderr(Stdio::null());
    if let Ok(mut child) = cmd.spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_account() {
        assert_eq!(clean_path("/Photos//2024/").unwrap(), "Photos/2024");
        assert_eq!(clean_path("").unwrap(), "");
        assert!(clean_path("Photos/../../etc").is_err());
        assert!(clean_path("./x").is_err());
        assert!(clean_path("a\nb").is_err());
        let cached = cached_path("dropbox-1", "Photos/DSCF0001.RAF").unwrap();
        assert!(cached.starts_with(cache_dir().join("dropbox-1")));
        assert!(cached_path("dropbox-1", "").is_err());
    }

    #[test]
    fn ids_are_unique_per_service() {
        let a = |id: &str| Account { id: id.into(), provider: "dropbox".into(), label: "Dropbox".into(), user: String::new() };
        assert_eq!(next_id(&[], "dropbox"), "dropbox-1");
        assert_eq!(next_id(&[a("dropbox-1"), a("dropbox-3")], "dropbox"), "dropbox-2");
    }

    #[test]
    fn every_service_is_listed_once() {
        let mut ids: Vec<_> = PROVIDERS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PROVIDERS.len());
        for want in ["google-drive", "onedrive", "dropbox", "github"] {
            assert!(provider(want).is_some(), "{want}");
        }
    }

    #[test]
    fn connect_events_are_tagged() {
        let e = ConnectEvent::DeviceCode { code: "ABCD-1234".into(), url: "https://github.com/login/device".into() };
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"{"event":"device_code","code":"ABCD-1234","url":"https://github.com/login/device"}"#);
    }
}
