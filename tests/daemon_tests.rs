use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn test_daemon_ping_and_clean_exit() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"))
        .arg("daemon")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start omastudio-engine daemon");

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let stdout = child.stdout.take().expect("Failed to open stdout");
    let mut reader = BufReader::new(stdout);

    // 1. Send Ping
    writeln!(stdin, "{{\"cmd\": \"ping\"}}").expect("Failed to write ping");
    stdin.flush().expect("Failed to flush stdin");

    let mut line = String::new();
    reader.read_line(&mut line).expect("Failed to read ping response");
    assert!(line.contains("\"success\":true"), "Response must be success: {}", line);
    assert!(line.contains("\"action\":\"ping\""), "Action must be ping: {}", line);
    assert!(line.contains("\"data\":\"pong\""), "Data must be pong: {}", line);

    // 2. Send Exit
    writeln!(stdin, "{{\"cmd\": \"exit\"}}").expect("Failed to write exit");
    stdin.flush().expect("Failed to flush stdin");

    let status = child.wait().expect("Failed to wait on daemon");
    assert!(status.success(), "Daemon must exit successfully");
}

#[test]
fn test_daemon_adjust_without_load_returns_error() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"))
        .arg("daemon")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start omastudio-engine daemon");

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let stdout = child.stdout.take().expect("Failed to open stdout");
    let mut reader = BufReader::new(stdout);

    // Send adjust before loading image
    writeln!(stdin, "{{\"cmd\": \"adjust\", \"recipe\": {{}}}}").expect("Failed to write adjust");
    stdin.flush().expect("Failed to flush stdin");

    let mut line = String::new();
    reader.read_line(&mut line).expect("Failed to read adjust response");
    assert!(line.contains("\"success\":false"), "Must return success:false when no image loaded: {}", line);
    assert!(line.contains("\"action\":\"adjust\""), "Action must be adjust: {}", line);

    // Send exit
    writeln!(stdin, "{{\"cmd\": \"exit\"}}").expect("Failed to write exit");
    stdin.flush().expect("Failed to flush stdin");

    let status = child.wait().expect("Failed to wait on daemon");
    assert!(status.success(), "Daemon must exit successfully");
}

#[test]
fn test_daemon_ai_jev_without_load_returns_error() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"))
        .arg("daemon")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start omastudio-engine daemon");

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let stdout = child.stdout.take().expect("Failed to open stdout");
    let mut reader = BufReader::new(stdout);

    // Send ai_jev before loading image
    writeln!(stdin, "{{\"cmd\": \"ai_jev\"}}").expect("Failed to write ai_jev");
    stdin.flush().expect("Failed to flush stdin");

    let mut line = String::new();
    reader.read_line(&mut line).expect("Failed to read ai_jev response");
    assert!(line.contains("\"success\":false"), "Must return success:false when no image loaded: {}", line);
    assert!(line.contains("\"action\":\"ai_jev\""), "Action must be ai_jev: {}", line);

    // Send exit
    writeln!(stdin, "{{\"cmd\": \"exit\"}}").expect("Failed to write exit");
    stdin.flush().expect("Failed to flush stdin");

    let status = child.wait().expect("Failed to wait on daemon");
    assert!(status.success(), "Daemon must exit successfully");
}

#[test]
fn test_daemon_batch_export_empty_or_valid() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"))
        .arg("daemon")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start omastudio-engine daemon");

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let stdout = child.stdout.take().expect("Failed to open stdout");
    let mut reader = BufReader::new(stdout);

    // Send batch_export without items
    writeln!(stdin, "{{\"cmd\": \"batch_export\", \"items\": []}}").expect("Failed to write batch_export");
    stdin.flush().expect("Failed to flush stdin");

    let mut line = String::new();
    reader.read_line(&mut line).expect("Failed to read batch_export response");
    assert!(line.contains("\"success\":false"), "Empty items must return success:false: {}", line);
    assert!(line.contains("\"action\":\"batch_export\""), "Action must be batch_export: {}", line);

    // Send exit
    writeln!(stdin, "{{\"cmd\": \"exit\"}}").expect("Failed to write exit");
    stdin.flush().expect("Failed to flush stdin");

    let status = child.wait().expect("Failed to wait on daemon");
    assert!(status.success(), "Daemon must exit successfully");
}

#[test]
fn test_daemon_anti_echo_loop_resilience() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"))
        .arg("daemon")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start omastudio-engine daemon");

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let stdout = child.stdout.take().expect("Failed to open stdout");
    let mut reader = BufReader::new(stdout);

    // 1. Send empty lines and whitespaces (should be ignored without emitting errors)
    writeln!(stdin).expect("Write newline");
    writeln!(stdin, "   \t  ").expect("Write whitespace");
    stdin.flush().expect("Flush stdin");

    // 2. Send malformed JSON
    writeln!(stdin, "{{ invalid json").expect("Write malformed json");
    stdin.flush().expect("Flush stdin");

    let mut line = String::new();
    reader.read_line(&mut line).expect("Read error line");
    assert!(line.contains("\"success\":false"), "Must return failure on invalid json");
    assert!(line.contains("\"action\":\"error\""), "Action should be error");

    // 3. Send unknown command
    writeln!(stdin, "{{\"cmd\": \"nonexistent_action_loop_test\"}}").expect("Write unknown cmd");
    stdin.flush().expect("Flush stdin");

    let mut line2 = String::new();
    reader.read_line(&mut line2).expect("Read unknown response");
    assert!(line2.contains("\"success\":false"), "Must return failure on unknown action");
    assert!(line2.contains("Unknown daemon command"), "Should state unknown daemon command");

    // 4. Send ping to ensure daemon is still fully responsive and unblocked
    writeln!(stdin, "{{\"cmd\": \"ping\"}}").expect("Write ping");
    stdin.flush().expect("Flush stdin");

    let mut line3 = String::new();
    reader.read_line(&mut line3).expect("Read ping");
    assert!(line3.contains("\"success\":true"), "Daemon must remain responsive");

    // 5. Clean exit
    writeln!(stdin, "{{\"cmd\": \"exit\"}}").expect("Write exit");
    stdin.flush().expect("Flush stdin");

    let status = child.wait().expect("Wait daemon");
    assert!(status.success(), "Daemon must exit cleanly");
}

#[test]
fn test_daemon_stdin_eof_graceful_termination() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"))
        .arg("daemon")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start omastudio-engine daemon");

    let stdin = child.stdin.take().expect("Failed to open stdin");
    // Explicitly drop stdin to simulate EOF
    drop(stdin);

    let status = child.wait().expect("Daemon should terminate on stdin EOF");
    assert!(status.success(), "Daemon must exit cleanly on EOF without hanging");
}


#[test]
fn test_daemon_answers_while_batch_export_runs() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"))
        .arg("daemon")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start omastudio-engine daemon");
    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("Failed to open stdout"));

    // The batch runs on its own thread: the ping is answered as well, in either order
    writeln!(stdin, r#"{{"cmd": "batch_export", "items": [{{"path": "/nonexistent/a.RAF"}}]}}"#).unwrap();
    writeln!(stdin, r#"{{"cmd": "ping"}}"#).unwrap();
    stdin.flush().unwrap();
    let mut lines = Vec::new();
    for _ in 0..2 {
        let mut line = String::new();
        reader.read_line(&mut line).expect("Failed to read a response");
        lines.push(line);
    }
    assert!(lines.iter().any(|l| l.contains("\"action\":\"ping\"")), "{lines:?}");
    let batch = lines.iter().find(|l| l.contains("\"action\":\"batch_export\"")).expect("batch result");
    assert!(batch.contains("\"failed\":1"), "{batch}");

    writeln!(stdin, r#"{{"cmd": "exit"}}"#).unwrap();
    stdin.flush().unwrap();
    assert!(child.wait().unwrap().success());
}

fn export_with_stdin(recipe_arg: &str, stdin_text: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"))
        .args(["export", "/nonexistent/photo.RAF", "--recipe", recipe_arg])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start omastudio-engine export");
    let mut stdin = child.stdin.take().unwrap();
    // A closed pipe is fine: the argument form never reads stdin
    let _ = stdin.write_all(stdin_text.as_bytes());
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn test_export_rejects_a_broken_recipe() {
    // A recipe that does not parse must not export the photo without its edits
    let out = export_with_stdin("{\"exposure\": ", "");
    assert!(out.contains("\"success\":false") && out.contains("Invalid recipe"), "{out}");
}

#[test]
fn test_export_reads_a_large_recipe_from_stdin() {
    // Larger than one command-line argument may be (128 KiB)
    let points: Vec<String> = (0..20_000).map(|i| format!("[{:.4},{:.4}]", i as f32 / 20_000.0, 0.5)).collect();
    let recipe = format!(
        r#"{{"exposure": 0.5, "layers": [{{"name": "brush", "shape": {{"type": "brush", "strokes": [{{"points": [{}]}}], "aspect": 1.0}}}}]}}"#,
        points.join(",")
    );
    assert!(recipe.len() > 128 << 10);
    let out = export_with_stdin("-", &format!("{recipe}\n"));
    assert!(out.contains("\"success\":false"), "{out}");
    assert!(!out.contains("Invalid recipe") && !out.contains("stdin"), "the recipe itself must be accepted: {out}");
}

#[test]
fn test_daemon_echoes_request_id_and_photo_path_and_keeps_export_non_blocking() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"))
        .arg("daemon")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start omastudio-engine daemon");
    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("Failed to open stdout"));

    // 1. Ping echoes request_id
    writeln!(stdin, r#"{{"cmd": "ping", "request_id": 42}}"#).unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["success"], true);
    assert_eq!(v["action"], "ping");
    assert_eq!(v["request_id"], 42);

    // 2. Load error echoes request_id and target photo_path
    writeln!(stdin, r#"{{"cmd": "load", "request_id": 43, "path": "/nonexistent/sample.RAF"}}"#).unwrap();
    stdin.flush().unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["success"], false);
    assert_eq!(v["action"], "load");
    assert_eq!(v["request_id"], 43);
    assert_eq!(v["photo_path"], "/nonexistent/sample.RAF");

    // 3. Batch export carries request_id through the bounded export worker while ping stays responsive
    writeln!(stdin, r#"{{"cmd": "batch_export", "request_id": 44, "items": [{{"path": "/nonexistent/b.RAF"}}]}}"#).unwrap();
    writeln!(stdin, r#"{{"cmd": "ping", "request_id": 45}}"#).unwrap();
    stdin.flush().unwrap();
    let mut responses = Vec::new();
    for _ in 0..2 {
        let mut l = String::new();
        reader.read_line(&mut l).unwrap();
        responses.push(serde_json::from_str::<serde_json::Value>(&l).unwrap());
    }
    let ping = responses.iter().find(|r| r["action"] == "ping").expect("ping response");
    assert_eq!(ping["request_id"], 45);
    let batch = responses.iter().find(|r| r["action"] == "batch_export").expect("batch_export response");
    assert_eq!(batch["request_id"], 44);
    assert_eq!(batch["data"]["failed"], 1);

    // 4. Exit echoes request_id
    writeln!(stdin, r#"{{"cmd": "exit", "request_id": 46}}"#).unwrap();
    stdin.flush().unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["action"], "exit");
    assert_eq!(v["request_id"], 46);
    assert!(child.wait().unwrap().success());
}

#[test]
fn test_daemon_detail_rejects_extreme_or_invalid_regions_and_renders_valid_ppm() {
    let dir = std::env::temp_dir().join(format!("omastudio_daemon_detail_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let img_path = dir.join("tiny.png");
    let img = image::RgbImage::from_pixel(32, 24, image::Rgb([255, 255, 255]));
    img.save(&img_path).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"))
        .arg("daemon")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start omastudio-engine daemon");
    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("Failed to open stdout"));

    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "cmd": "load", "request_id": 1, "path": img_path.to_string_lossy() })
    )
    .unwrap();
    stdin.flush().unwrap();

    // Wait for both `load` and background `full_ready`
    let mut got_load = false;
    let mut got_full = false;
    while !(got_load && got_full) {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["action"] == "load" {
            assert_eq!(v["success"], true);
            assert_eq!(v["request_id"], 1);
            assert_eq!(v["photo_path"], img_path.to_string_lossy().as_ref());
            got_load = true;
        } else if v["action"] == "full_ready" {
            assert_eq!(v["photo_path"], img_path.to_string_lossy().as_ref());
            got_full = true;
        }
    }

    // Huge rw (previously produced vw=0, vh=0 -> NaN in region) must be cleanly rejected
    writeln!(
        stdin,
        r#"{{"cmd": "detail", "request_id": 2, "region": [0.0, 0.0, 1e12, 1.0], "out_width": 16, "out_height": 16}}"#
    )
    .unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["success"], false);
    assert_eq!(v["action"], "detail");
    assert_eq!(v["request_id"], 2);
    assert_eq!(v["photo_path"], img_path.to_string_lossy().as_ref());

    // Valid detail with highlight_mask=true must render red clipping overlay on pure white image
    writeln!(
        stdin,
        r#"{{"cmd": "detail", "request_id": 3, "region": [0.0, 0.0, 1.0, 1.0], "out_width": 16, "out_height": 12, "highlight_mask": true}}"#
    )
    .unwrap();
    stdin.flush().unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["success"], true, "valid detail failed: {v}");
    assert_eq!(v["request_id"], 3);
    assert_eq!(v["photo_path"], img_path.to_string_lossy().as_ref());
    let ppm_path = v["data"]["image"].as_str().unwrap();
    let ppm = std::fs::read(ppm_path).unwrap();
    // Skip P6 header (3 newlines) and check first pixel is highlight-clipped red (255, 0, 0)
    let mut nl = 0;
    let header_end = ppm.iter().position(|&b| {
        if b == b'\n' {
            nl += 1;
        }
        nl == 3
    }).unwrap() + 1;
    assert_eq!(&ppm[header_end..header_end + 3], &[255, 0, 0], "detail must apply highlight_mask overlay");

    writeln!(stdin, r#"{{"cmd": "exit"}}"#).unwrap();
    stdin.flush().unwrap();
    assert!(child.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(&dir);
}
