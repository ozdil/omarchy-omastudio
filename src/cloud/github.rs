//! GitHub through the GitHub CLI (`gh`).
//!
//! Signing in uses `gh auth login --web`: GitHub's device flow, where the user types a
//! short code on github.com. `gh` keeps the token in the system keyring. OmaStudio
//! reuses an existing `gh` sign-in and never logs it out.
//!
//! Paths are `owner/repo/dir/file`; the top level lists the user's repositories.
//! Uploads go to the private repository `<user>/OmaStudio`, created on connecting.
//! The contents API used here handles files up to 100 MB (GitHub's own limit).

use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use super::{ConnectEvent, Entry, UPLOAD_FOLDER, last_error, open_browser, run, run_streaming, tool, which};
use crate::security;

const HOST: &str = "github.com";
const QUICK_TIMEOUT: Duration = Duration::from_secs(60);
/// GitHub's device codes expire after 15 minutes
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const MAX_FILE_BYTES: u64 = 100 << 20;

fn gh(args: &[&str]) -> std::process::Command {
    let mut cmd = tool("gh");
    cmd.args(args).env("GH_PROMPT_DISABLED", "1").env("NO_COLOR", "1");
    cmd
}

fn require_gh() -> Result<(), String> {
    if which("gh") { Ok(()) } else { Err("The GitHub CLI is not installed (sudo pacman -S github-cli)".into()) }
}

/// The one-time code and verification URL `gh` prints during the device flow.
fn device_code(line: &str) -> Option<String> {
    let at = line.find("one-time code:")?;
    let code = line[at + "one-time code:".len()..].trim();
    let ok = code.len() == 9 && code.as_bytes()[4] == b'-' && code.chars().all(|c| c == '-' || c.is_ascii_alphanumeric());
    ok.then(|| code.to_string())
}

fn device_url(line: &str) -> Option<String> {
    let start = line.find("https://github.com/login/device")?;
    Some(line[start..].chars().take_while(|c| !c.is_whitespace()).collect())
}

/// Signs in (unless `gh` already is), makes sure the upload repository exists and
/// returns the user's login.
pub fn connect(progress: &mut dyn FnMut(ConnectEvent)) -> Result<String, String> {
    require_gh()?;
    let (code, _, _) = run(gh(&["auth", "status", "--hostname", HOST]), QUICK_TIMEOUT)?;
    if code != 0 {
        let mut pending_code: Option<String> = None;
        let mut on_line = |line: &str| {
            if let Some(c) = device_code(line) {
                pending_code = Some(c);
            } else if let (Some(url), Some(c)) = (device_url(line), pending_code.take()) {
                open_browser(&url);
                progress(ConnectEvent::DeviceCode { code: c, url });
            }
        };
        let login = gh(&["auth", "login", "--hostname", HOST, "--web", "--git-protocol", "https", "--skip-ssh-key"]);
        let (code, _, err) = run_streaming(login, SIGN_IN_TIMEOUT, &mut on_line)?;
        if code != 0 {
            return Err(format!("GitHub sign-in failed: {}", last_error(&err, "gh failed")));
        }
    }
    let (code, out, err) = run(gh(&["api", "user", "--jq", ".login"]), QUICK_TIMEOUT)?;
    let login = out.trim().to_string();
    if code != 0 || login.is_empty() || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("Cannot read the GitHub user: {}", last_error(&err, "gh failed")));
    }
    let repo = format!("{login}/{UPLOAD_FOLDER}");
    let (exists, _, _) = run(gh(&["api", &format!("repos/{repo}"), "--silent"]), QUICK_TIMEOUT)?;
    if exists != 0 {
        let create = gh(&[
            "repo", "create", UPLOAD_FOLDER, "--private", "--add-readme",
            "--description", "Photos exported from OmaStudio",
        ]);
        let (code, _, err) = run(create, QUICK_TIMEOUT)?;
        if code != 0 {
            return Err(format!("Signed in, but the private repository {repo} could not be created: {}", last_error(&err, "gh failed")));
        }
    }
    Ok(login)
}

/// Percent-encodes one path segment for the REST API.
fn encode(segment: &str) -> String {
    let mut out = String::new();
    for b in segment.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `repos/owner/repo/contents/a/b` for `owner/repo/a/b`.
fn contents_endpoint(path: &str) -> Result<String, String> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return Err(format!("Not a repository path: {path}"));
    }
    let rest: Vec<String> = parts[2..].iter().map(|p| encode(p)).collect();
    Ok(format!("repos/{}/{}/contents/{}", encode(parts[0]), encode(parts[1]), rest.join("/")))
}

#[derive(Deserialize)]
struct ContentItem {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    size: u64,
}

pub fn list(path: &str) -> Result<Vec<Entry>, String> {
    require_gh()?;
    if path.is_empty() {
        let args = ["api", "--paginate", "user/repos?per_page=100&sort=updated", "--jq", ".[].full_name"];
        let (code, out, err) = run(gh(&args), QUICK_TIMEOUT * 2)?;
        if code != 0 {
            return Err(format!("Cannot list the repositories: {}", last_error(&err, "gh failed")));
        }
        return Ok(out
            .lines()
            .map(str::trim)
            .filter(|l| l.split('/').count() == 2)
            .map(|full| Entry { name: full.to_string(), path: full.to_string(), is_dir: true, size: 0 })
            .collect());
    }
    if path.split('/').count() == 1 {
        return Err(format!("Not a repository path: {path}"));
    }
    let (code, out, err) = run(gh(&["api", &contents_endpoint(path)?]), QUICK_TIMEOUT)?;
    if code != 0 {
        return Err(format!("Cannot list the folder: {}", last_error(&err, "gh failed")));
    }
    let items: Vec<ContentItem> = serde_json::from_str(&out).map_err(|_| format!("{path} is not a folder"))?;
    Ok(items
        .into_iter()
        .filter(|i| (i.kind == "dir" || i.kind == "file") && !i.name.starts_with('.') && !i.name.contains('/'))
        .map(|i| Entry { path: format!("{path}/{}", i.name), is_dir: i.kind == "dir", size: i.size, name: i.name })
        .collect())
}

pub fn fetch(path: &str, local: &Path) -> Result<(), String> {
    require_gh()?;
    if path.split('/').count() < 3 {
        return Err(format!("Not a file: {path}"));
    }
    let endpoint = contents_endpoint(path)?;
    let (code, out, err) = run(gh(&["api", &endpoint, "--jq", ".size"]), QUICK_TIMEOUT)?;
    if code != 0 {
        return Err(format!("Cannot open the file: {}", last_error(&err, "gh failed")));
    }
    let size: u64 = out.trim().parse().map_err(|_| format!("{path} is not a file"))?;
    if size > MAX_FILE_BYTES {
        return Err(format!("{path} is larger than 100 MB"));
    }
    if std::fs::symlink_metadata(local).is_ok_and(|m| m.is_file() && m.len() == size) {
        return Ok(());
    }
    let partial = local.with_extension("partial");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(&partial)
        .map_err(|e| format!("Cannot write {}: {e}", partial.display()))?;
    let cmd = gh(&["api", "-H", "Accept: application/vnd.github.raw", &endpoint]);
    let written = security::run_bounded_command_stream_to_file(cmd, &mut file, TRANSFER_TIMEOUT, MAX_FILE_BYTES as usize + 1);
    match written {
        Ok(n) if n as u64 == size => std::fs::rename(&partial, local).map_err(|e| format!("Download failed: {e}")),
        Ok(_) => {
            let _ = std::fs::remove_file(&partial);
            Err("Download failed: the file arrived incomplete".into())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(format!("Download failed: {e}"))
        }
    }
}

pub fn upload(file: &Path, target: &str) -> Result<(), String> {
    require_gh()?;
    if target.split('/').count() < 3 {
        return Err(format!("Not a file path inside a repository: {target}"));
    }
    let size = std::fs::metadata(file).map_err(|e| e.to_string())?.len();
    if size > MAX_FILE_BYTES {
        return Err(format!("{} is larger than 100 MB, GitHub's limit", file.display()));
    }
    let data = security::read_secure_file(file, MAX_FILE_BYTES as usize).map_err(|e| format!("{}: {e}", file.display()))?;
    let endpoint = contents_endpoint(target)?;
    // Replacing a file needs the current one's blob id
    let sha = match run(gh(&["api", &endpoint, "--jq", ".sha"]), QUICK_TIMEOUT)? {
        (0, out, _) if !out.trim().is_empty() => Some(out.trim().to_string()),
        _ => None,
    };
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("photo");
    let mut body = serde_json::json!({ "message": format!("Add {name} from OmaStudio"), "content": base64(&data) });
    if let Some(sha) = sha {
        body["sha"] = sha.into();
    }
    let request = file_beside_cache("upload.json");
    security::atomic_write_secure(&request, body.to_string().as_bytes()).map_err(|e| e.to_string())?;
    let result = run(gh(&["api", "-X", "PUT", &endpoint, "--input", &request.to_string_lossy(), "--silent"]), TRANSFER_TIMEOUT);
    let _ = std::fs::remove_file(&request);
    let (code, _, err) = result?;
    if code != 0 {
        return Err(format!("Upload failed: {}", last_error(&err, "gh failed")));
    }
    Ok(())
}

fn file_beside_cache(name: &str) -> std::path::PathBuf {
    super::cache_dir().join(format!(".{}-{name}", std::process::id()))
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_device_flow() {
        assert_eq!(device_code("! First copy your one-time code: AB12-CD34").as_deref(), Some("AB12-CD34"));
        assert_eq!(device_code("! First copy your one-time code: nonsense"), None);
        assert_eq!(
            device_url("Open this URL to continue in your web browser: https://github.com/login/device").as_deref(),
            Some("https://github.com/login/device")
        );
    }

    #[test]
    fn encodes_paths() {
        assert_eq!(contents_endpoint("ozdil/photos/2024 trip/DSCF1.RAF").unwrap(), "repos/ozdil/photos/contents/2024%20trip/DSCF1.RAF");
        assert_eq!(contents_endpoint("ozdil/photos").unwrap(), "repos/ozdil/photos/contents/");
        assert!(contents_endpoint("ozdil").is_err());
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
