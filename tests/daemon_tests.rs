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
