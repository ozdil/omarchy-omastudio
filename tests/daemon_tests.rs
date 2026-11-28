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
    let status = child.wait().unwrap();
    assert!(status.success(), "daemon exited with {status:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_daemon_memfd_preview_ring_zero_disk_writes() {
    use omastudio_engine::preview_shm::{
        ColorSpaceTag, ENV_PREVIEW_FD, PixelFormat, PreviewRingReader, SlotKind,
        create_preview_socketpair, parse_provider_id,
    };
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;

    let dir = std::env::temp_dir().join(format!("omastudio_daemon_memfd_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let runtime_dir = dir.join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let img_path = dir.join("tiny_memfd.png");
    let img = image::RgbImage::from_pixel(32, 24, image::Rgb([40, 120, 210]));
    img.save(&img_path).unwrap();

    let (parent_fd, child_fd) = create_preview_socketpair().expect("socketpair");
    let child_raw = child_fd.as_raw_fd();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_omastudio-engine"));
    cmd.arg("daemon")
        .env(ENV_PREVIEW_FD, child_raw.to_string())
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .env_remove("OMASTUDIO_PREVIEW_MODE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    // SAFETY: `fcntl(F_SETFD, 0)` is async-signal-safe and clears FD_CLOEXEC solely on the child socketpair half.
    unsafe {
        cmd.pre_exec(move || {
            if libc::fcntl(child_raw, libc::F_SETFD, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = cmd.spawn().expect("Failed to start omastudio-engine daemon");
    drop(child_fd);

    // Complete SCM_RIGHTS handshake with daemon
    let ring = PreviewRingReader::recv_and_ack(parent_fd.as_raw_fd())
        .expect("recv_and_ack")
        .expect("memfd ring negotiated");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));

    writeln!(
        stdin,
        "{}",
        serde_json::json!({ "cmd": "load", "request_id": 10, "path": img_path.to_string_lossy() })
    )
    .unwrap();
    stdin.flush().unwrap();

    let mut got_load = false;
    let mut got_full = false;
    while !(got_load && got_full) {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["action"] == "load" {
            assert_eq!(v["success"], true);
            got_load = true;
        } else if v["action"] == "full_ready" {
            got_full = true;
        }
    }

    // 1. Interactive adjust must publish to memfd slot 0 (no .ppm file)
    writeln!(stdin, r#"{{"cmd": "adjust", "request_id": 11, "recipe": {{}}}}"#).unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["success"], true, "adjust failed: {v}");
    let url = v["data"]["image"].as_str().unwrap();
    assert!(url.starts_with("image://omastudio/preview/0/"), "unexpected url: {url}");
    assert_eq!(v["data"]["slot"], 0);
    assert_eq!(v["data"]["width"], 16);
    assert_eq!(v["data"]["height"], 12);
    assert_eq!(v["data"]["stride"], 64);
    assert_eq!(v["data"]["format"], "rgba8888");
    assert_eq!(v["data"]["color_space"], "srgb");

    let (kind, slot, generation) = parse_provider_id(url.strip_prefix("image://omastudio/").unwrap()).unwrap();
    assert_eq!((kind, slot), (SlotKind::Preview, 0));
    let frame = ring.read_frame(kind, slot).expect("read preview frame from memfd");
    assert_eq!(frame.generation, generation);
    assert_eq!((frame.width, frame.height, frame.stride), (16, 12, 64));
    assert_eq!(frame.format, PixelFormat::Rgba8888);
    assert_eq!(frame.color_space, ColorSpaceTag::Srgb);
    assert_eq!(frame.pixels.len(), 16 * 12 * 4);
    assert_eq!(frame.pixels[3], 255);

    // 2. Compare reference & detail & loupe must also publish via memfd
    writeln!(stdin, r#"{{"cmd": "reference", "request_id": 12, "recipe": {{}}}}"#).unwrap();
    writeln!(
        stdin,
        r#"{{"cmd": "detail", "request_id": 13, "region": [0.0, 0.0, 1.0, 1.0], "out_width": 16, "out_height": 12}}"#
    )
    .unwrap();
    writeln!(
        stdin,
        r#"{{"cmd": "detail", "tag": "loupe", "request_id": 14, "region": [0.0, 0.0, 1.0, 1.0], "out_width": 16, "out_height": 12}}"#
    )
    .unwrap();
    stdin.flush().unwrap();

    for expected_prefix in [
        "image://omastudio/reference/0/",
        "image://omastudio/detail/0/",
        "image://omastudio/loupe/0/",
    ] {
        line.clear();
        reader.read_line(&mut line).unwrap();
        let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(resp["success"], true, "unexpected failure: {resp}");
        let img_url = resp["data"]["image"].as_str().unwrap();
        assert!(
            img_url.starts_with(expected_prefix),
            "expected {expected_prefix}, got {img_url}"
        );
        let (k, s, g) = parse_provider_id(img_url.strip_prefix("image://omastudio/").unwrap()).unwrap();
        let f = ring.read_frame(k, s).expect("memfd frame readable");
        assert_eq!(f.generation, g);
    }

    // 3. Verify zero .ppm preview files were written into the isolated runtime directory
    fn has_ppm(root: &std::path::Path) -> bool {
        let Ok(rd) = std::fs::read_dir(root) else { return false };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() && has_ppm(&p) {
                return true;
            }
            if p.extension().is_some_and(|e| e == "ppm") {
                return true;
            }
        }
        false
    }
    assert!(!has_ppm(&runtime_dir), "memfd mode must not write .ppm preview files");

    writeln!(stdin, r#"{{"cmd": "exit"}}"#).unwrap();
    stdin.flush().unwrap();
    assert!(child.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(&dir);
}

