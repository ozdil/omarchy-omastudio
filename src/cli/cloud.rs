//! Cloud storage CLI commands (`cloud status|connect|list|fetch|upload|disconnect`).

use std::path::Path;

use omastudio_engine::{
    cloud,
    raw::{self, RawImage},
};

use super::inspect::thumbnail_path_for;
use super::{ResponseWrapper, print_json, print_line, read_stdin_line};

pub fn run(args: &[String]) {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let has = |name: &str| args.iter().any(|a| a == name);
    let positional: Vec<&String> = {
        let mut out = Vec::new();
        let mut skip = false;
        for a in args.iter().skip(1) {
            if skip {
                skip = false;
            } else if a == "--client-secret-stdin" {
                // Boolean flag: takes no value argument
            } else if a.starts_with("--") {
                skip = true;
            } else {
                out.push(a);
            }
        }
        out
    };
    let arg = |i: usize, usage: &str| {
        positional
            .get(i)
            .map(|s| s.as_str())
            .ok_or_else(|| format!("Usage: cloud {usage}"))
    };
    let result: Result<serde_json::Value, String> = (|| match args.first().map(String::as_str) {
        Some("status") | None => serde_json::to_value(cloud::status()).map_err(|e| e.to_string()),
        Some("connect") => {
            let service = arg(0, "connect <service>")?;
            if has("--client-secret") {
                return Err(
                    "Passing --client-secret on the command line is disallowed; use --client-secret-stdin and supply the secret on stdin".into(),
                );
            }
            let client_secret = if has("--client-secret-stdin") {
                read_stdin_line(4096).map_err(|e| format!("Cannot read client secret from stdin: {e}"))?
            } else {
                String::new()
            };
            let client = cloud::OAuthClient {
                client_id: flag("--client-id").unwrap_or_default(),
                client_secret,
            };
            let account = cloud::connect(service, &client, &mut |event| {
                if let Ok(line) = serde_json::to_string(&event) {
                    print_line(&line);
                }
            })?;
            serde_json::to_value(account).map_err(|e| e.to_string())
        }
        Some("disconnect") => {
            cloud::disconnect(arg(0, "disconnect <account>")?)?;
            Ok(serde_json::json!({ "disconnected": true }))
        }
        Some("list") => {
            let account = arg(0, "list <account> [path]")?;
            let path = positional.get(1).map(|s| s.as_str()).unwrap_or("");
            let items: Vec<serde_json::Value> = cloud::list(account, path)?
                .into_iter()
                .filter(|e| {
                    e.is_dir
                        || Path::new(&e.name)
                            .extension()
                            .and_then(|x| x.to_str())
                            .is_some_and(|x| raw::is_supported_extension(&x.to_lowercase()))
                })
                .map(|e| {
                    let local = if e.is_dir { None } else { cloud::cached_path(account, &e.path).ok() };
                    let thumbnail = local
                        .as_ref()
                        .filter(|p| p.is_file())
                        .map(|p| thumbnail_path_for(p))
                        .filter(|t| t.is_file())
                        .map(|t| t.to_string_lossy().to_string())
                        .unwrap_or_default();
                    serde_json::json!({
                        "name": e.name,
                        "path": local.map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| e.path.clone()),
                        "remote_path": e.path,
                        "is_dir": e.is_dir,
                        "is_remote": !e.is_dir,
                        "size": e.size,
                        "thumbnail": thumbnail,
                    })
                })
                .collect();
            Ok(serde_json::json!({ "account": account, "path": cloud::clean_path(path)?, "items": items }))
        }
        Some("fetch") => {
            let local = cloud::fetch(arg(0, "fetch <account> <path>")?, arg(1, "fetch <account> <path>")?)?;
            let thumb = thumbnail_path_for(&local);
            if !thumb.exists()
                && let Ok(raw) = RawImage::open(&local)
            {
                let _ = raw.extract_thumbnail(&thumb);
            }
            let thumbnail = if thumb.is_file() {
                thumb.to_string_lossy().to_string()
            } else {
                String::new()
            };
            Ok(serde_json::json!({ "local_path": local, "thumbnail": thumbnail }))
        }
        Some("upload") => {
            let account = arg(0, "upload <account> <file> [dir]")?;
            let file = arg(1, "upload <account> <file> [dir]")?;
            let remote = cloud::upload(account, Path::new(file), positional.get(2).map(|s| s.as_str()))?;
            Ok(serde_json::json!({ "remote_path": remote }))
        }
        Some(other) => Err(format!("Unknown cloud command: {other}")),
    })();
    match result {
        Ok(v) => print_json(&ResponseWrapper::ok(v)),
        Err(e) => {
            print_json::<()>(&ResponseWrapper::err(e));
            std::process::exit(1);
        }
    }
}
