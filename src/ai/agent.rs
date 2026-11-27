//! Omarchy's default AI agent as a provider for the edit assistant.
//!
//! Omarchy keeps the agent the user picked (`omarchy-default-agent`) in
//! `~/.config/omarchy/defaults/agent`. Agents with a headless mode that can look at an
//! image are run with the photo in a private directory and the request as their
//! prompt; they answer with the JSON object the assistant expects. The agent uses its
//! own sign-in, so no key is stored here.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::security::{self, run_bounded_command_with_stdin, secure_command};

/// Agents that can answer headless with an image: (Omarchy agent name, program).
const SUPPORTED: &[(&str, &str)] = &[("claude", "claude"), ("codex", "codex")];

const TIMEOUT: Duration = Duration::from_secs(300);

fn config_dir() -> PathBuf {
    crate::paths::xdg_config_dir()
}

/// The agent Omarchy uses by default, if one is set.
pub fn default_agent() -> Option<String> {
    let raw = security::read_secure_file(&config_dir().join("omarchy/defaults/agent"), 256).ok()?;
    let name = String::from_utf8_lossy(&raw)
        .lines()
        .next()?
        .trim()
        .to_string();
    (!name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then_some(name)
}

/// `program` on PATH, or in ~/.local/bin where agent installers put it.
fn find_program(program: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(Path::new(&home).join(".local/bin"));
    }
    dirs.into_iter()
        .map(|d| d.join(program))
        .find(|p| p.is_file())
}

/// The default agent and its program, when the agent is supported and installed.
pub fn available() -> Option<(String, PathBuf)> {
    let agent = default_agent()?;
    let (_, program) = SUPPORTED.iter().find(|(name, _)| *name == agent)?;
    Some((agent, find_program(program)?))
}

/// Claude Code sees only its Read tool, may read only the photo, and loads none of
/// the user's settings, permission rules, hooks or MCP servers: an instruction hidden
/// in the photo cannot make it read other files or run anything.
const CLAUDE_ARGS: &[&str] = &[
    "-p",
    "--output-format",
    "json",
    "--tools",
    "Read",
    "--allowedTools",
    "Read(./photo.jpg)",
    "--permission-mode",
    "dontAsk",
    "--setting-sources",
    "",
    "--strict-mcp-config",
    "--max-turns",
    "4",
];

/// Environment variables forwarded to headless AI agents (`claude`, `codex`) so keychain,
/// API key auth, XDG paths, custom CAs, and proxies work without exposing loader variables.
const AGENT_PASSTHROUGH_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "OPENROUTER_API_KEY",
    "GEMINI_API_KEY",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_RUNTIME_DIR",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
];

fn apply_agent_env(cmd: &mut std::process::Command) {
    for &key in AGENT_PASSTHROUGH_ENV {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            cmd.env(key, val);
        }
    }
}

/// Shows the agent the photo and returns the JSON object it answers with.
pub fn ask_json(system: &str, jpeg: &[u8], text: &str, schema: &Value) -> Result<Value, String> {
    let Some((agent, program)) = available() else {
        return Err(match default_agent() {
            Some(a) => format!(
                "Omarchy's default agent {a} cannot be used for photo edits; choose claude or codex with omarchy-default-agent"
            ),
            None => {
                "Omarchy has no default AI agent; pick one with omarchy-default-agent".to_string()
            }
        });
    };
    let guard = security::TempDirGuard::create_private(&security::get_secure_preview_dir(), "omastudio_agent")
        .map_err(|e| e.to_string())?;
    run(&agent, &program, guard.path(), system, jpeg, text, schema)
}

fn run(
    agent: &str,
    program: &Path,
    dir: &Path,
    system: &str,
    jpeg: &[u8],
    text: &str,
    schema: &Value,
) -> Result<Value, String> {
    security::atomic_write_user(&dir.join("photo.jpg"), jpeg).map_err(|e| e.to_string())?;
    let prompt = format!(
        "{system}\n\nThe photo is the file photo.jpg in the current directory; look at it before answering.\n\n{text}\n\n\
         Answer with only a JSON object that matches this JSON schema, without code fences or other text:\n{schema}"
    );
    let mut cmd = secure_command(&program.to_string_lossy());
    apply_agent_env(&mut cmd);
    cmd.current_dir(dir);
    match agent {
        "claude" => {
            cmd.args(CLAUDE_ARGS).arg("--")
            .arg(&prompt);
        }
        _ => {
            cmd.args([
                "exec",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "--image",
                "photo.jpg",
                "--output-last-message",
                "answer.txt",
                "--",
            ])
            .arg(&prompt);
        }
    }
    let (code, out, err) = run_bounded_command_with_stdin(cmd, TIMEOUT, None)
        .map_err(|e| format!("Cannot run {agent}: {e}"))?;
    if code != 0 {
        let msg = String::from_utf8_lossy(if err.is_empty() { &out } else { &err })
            .trim()
            .chars()
            .take(300)
            .collect::<String>();
        return Err(format!("{agent} failed: {msg}"));
    }
    let answer = if agent == "claude" {
        let v: Value =
            serde_json::from_slice(&out).map_err(|_| format!("Unexpected answer from {agent}"))?;
        if v.get("is_error").and_then(Value::as_bool) == Some(true) {
            return Err(format!(
                "{agent}: {}",
                v.get("result").and_then(Value::as_str).unwrap_or("error")
            ));
        }
        v.get("result")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    } else {
        String::from_utf8_lossy(
            &security::read_secure_file(&dir.join("answer.txt"), 1 << 20)
                .map_err(|e| e.to_string())?,
        )
        .into_owned()
    };
    super::provider::parse_json_object(&answer)
        .ok_or_else(|| format!("{agent}'s answer was not valid JSON"))
}

#[cfg(test)]
mod tests {
    use super::CLAUDE_ARGS;

    fn value_of(flag: &str) -> Option<&'static str> {
        CLAUDE_ARGS.iter().position(|a| *a == flag).and_then(|i| CLAUDE_ARGS.get(i + 1).copied())
    }

    #[test]
    fn claude_is_confined_to_the_photo() {
        assert_eq!(value_of("--tools"), Some("Read"));
        assert_eq!(value_of("--allowedTools"), Some("Read(./photo.jpg)"));
        assert_eq!(value_of("--permission-mode"), Some("dontAsk"));
        // No user, project or local settings: their allow rules, hooks and MCP servers
        assert_eq!(value_of("--setting-sources"), Some(""));
        assert!(CLAUDE_ARGS.contains(&"--strict-mcp-config"));
        assert!(!CLAUDE_ARGS.iter().any(|a| a.contains("bypassPermissions")));
    }
}
