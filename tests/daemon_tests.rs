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

