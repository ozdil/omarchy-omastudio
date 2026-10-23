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
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config")
        })
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
    let dir =
        security::get_secure_preview_dir().join(format!("omastudio_agent_{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = run(&agent, &program, &dir, system, jpeg, text, schema);
    let _ = std::fs::remove_dir_all(&dir);
    result
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
    cmd.current_dir(dir);
    match agent {
        "claude" => {
            cmd.args([
                "-p",
                "--output-format",
                "json",
                "--allowedTools",
                "Read",
                "--max-turns",
                "4",
                "--",
            ])
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
