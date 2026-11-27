use std::fs::{self};
use std::io::{self, Read, Write};
use std::os::unix::io::RawFd;
use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

/// Monotonic per-process counter keeping concurrent staging file names unique
static STAGING_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Whitelisted environment variables for subprocess execution
pub const WHITELISTED_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LC_ALL",
    "LANG",
    "XDG_RUNTIME_DIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
];

/// Maximum buffer size for command output to prevent memory overrun (1 MiB ceiling for large directory listings)
pub const MAX_COMMAND_OUTPUT_BYTES: usize = 1024 * 1024;

/// Cleans and configures a Command according to Omarchy security standards:
/// - Isolated process group (cmd.process_group(0))
/// - Cleared environment variables with whitelist only
pub fn secure_command(program: &str) -> Command {
    secure_command_with_mem_limit(program, None)
}

/// Returns the real UID of the current process.
pub fn current_uid() -> libc::uid_t {
    // SAFETY: libc::getuid is thread-safe and infallible on Unix.
    unsafe { libc::getuid() }
}

/// Escapes a value for use inside a double-quoted curl `-K` config string.
/// Rejects CR, LF, and NUL bytes that could break config syntax or inject headers.
pub fn escape_curl_config_value(s: &str) -> Result<String, String> {
    if s.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
        return Err("curl config value contains CR, LF, or NUL".to_string());
    }
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    Ok(out)
}

/// Cleans and configures a Command according to Omarchy security standards:
/// - Isolated process group (cmd.process_group(0))
/// - Cleared environment variables with whitelist only
/// - Optional virtual memory limit (RLIMIT_AS / address space cap) via pre_exec
pub fn secure_command_with_mem_limit(program: &str, max_memory_bytes: Option<u64>) -> Command {
    let mut cmd = Command::new(program);
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd.env_clear();

    for &var in WHITELISTED_ENV_VARS {
        if let Ok(val) = std::env::var(var) {
            cmd.env(var, val);
        }
    }
    cmd.env("LC_ALL", "C");
    cmd.env("GIT_TERMINAL_PROMPT", "0");

    if let Some(mem_bytes) = max_memory_bytes {
        // SAFETY: closure runs between fork and exec and only calls async-signal-safe setrlimit.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(&mut cmd, move || {
                let limit = libc::rlimit {
                    rlim_cur: mem_bytes as libc::rlim_t,
                    rlim_max: mem_bytes as libc::rlim_t,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    cmd
}

/// Reaps a process group unconditionally with SIGTERM followed by SIGKILL
pub fn reap_process_group(child: &mut Child) {
    let pid = child.id() as i32;
    if pid <= 1 {
        return;
    }

    // SAFETY: pid is verified > 1, libc kill with negative pid targets the process group
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }

    // Grace period for cooperative shutdown (5-10ms)
    thread::sleep(Duration::from_millis(8));

    // Forceful kill
    // SAFETY: pid is verified > 1
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }

    // Prevent zombies
    let _ = child.wait();
}

/// RAII Guard ensuring process groups are terminated even upon panic or early return
pub struct ProcessGroupGuard<'a> {
    child: &'a mut Child,
    active: bool,
}

impl<'a> ProcessGroupGuard<'a> {
    pub fn new(child: &'a mut Child) -> Self {
        Self {
            child,
            active: true,
        }
    }

    pub fn defuse(mut self) {
        self.active = false;
    }
}

impl<'a> Drop for ProcessGroupGuard<'a> {
    fn drop(&mut self) {
        if self.active {
            reap_process_group(self.child);
        }
    }
}

fn set_nonblocking(fd: RawFd) {
    // SAFETY: fd is an open pipe descriptor owned by the child process.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputCapMode {
    FailOnOverrun,
    DrainExcess,
}

/// Runs a command with a monotonic deadline, non-blocking I/O polling, and buffer limit
pub fn run_bounded_command(
    cmd: Command,
    timeout: Duration,
) -> io::Result<(i32, Vec<u8>, Vec<u8>)> {
    run_bounded_command_with_stdin(cmd, timeout, None)
}

/// Runs a command with optional stdin data, a monotonic deadline, single `libc::poll()`
/// I/O multiplexing across stdin/stdout/stderr, and a strict output buffer cap.
pub fn run_bounded_command_with_stdin(
    cmd: Command,
    timeout: Duration,
    stdin_data: Option<&[u8]>,
) -> io::Result<(i32, Vec<u8>, Vec<u8>)> {
    run_bounded_command_inner(cmd, timeout, stdin_data, OutputCapMode::FailOnOverrun, None)
}

/// Runs a command with a monotonic deadline while invoking `on_stderr_line` for each
/// line emitted on stderr. Retains up to `MAX_COMMAND_OUTPUT_BYTES` for both stdout and
/// stderr while continuing to drain any excess output so the child never stalls on a full pipe.
pub fn run_bounded_command_streaming(
    cmd: Command,
    timeout: Duration,
    on_stderr_line: &mut dyn FnMut(&str),
) -> io::Result<(i32, Vec<u8>, Vec<u8>)> {
    run_bounded_command_inner(cmd, timeout, None, OutputCapMode::DrainExcess, Some(on_stderr_line))
}

fn run_bounded_command_inner(
    mut cmd: Command,
    timeout: Duration,
    stdin_data: Option<&[u8]>,
    cap_mode: OutputCapMode,
    mut on_stderr_line: Option<&mut dyn FnMut(&str)>,
) -> io::Result<(i32, Vec<u8>, Vec<u8>)> {
    use std::os::unix::io::AsRawFd;

    const MAX_STREAM_LINE_BYTES: usize = 64 * 1024;

    if stdin_data.is_some() {
        cmd.stdin(std::process::Stdio::piped());
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    let guard = ProcessGroupGuard::new(&mut child);
    let deadline = Instant::now() + timeout;

    let mut stdin = if stdin_data.is_some_and(|d| !d.is_empty()) {
        guard.child.stdin.take()
    } else {
        // Close immediately so empty stdin sends EOF rather than holding the pipe open.
        drop(guard.child.stdin.take());
        None
    };
    let mut stdout = guard.child.stdout.take();
    let mut stderr = guard.child.stderr.take();

    if let Some(ref p) = stdin {
        set_nonblocking(p.as_raw_fd());
    }
    if let Some(ref p) = stdout {
        set_nonblocking(p.as_raw_fd());
    }
    if let Some(ref p) = stderr {
        set_nonblocking(p.as_raw_fd());
    }

    let mut stdin_pos = 0usize;
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let mut stderr_line_buf = Vec::new();
    let mut temp_chunk = [0u8; 8192];

    while stdin.is_some() || stdout.is_some() || stderr.is_some() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Command exceeded monotonic deadline",
            ));
        }

        let mut pollfds = [libc::pollfd { fd: -1, events: 0, revents: 0 }; 3];
        let mut nfds = 0usize;
        let mut stdin_idx = None;
        let mut stdout_idx = None;
        let mut stderr_idx = None;

        if let Some(ref p) = stdin {
            stdin_idx = Some(nfds);
            pollfds[nfds] = libc::pollfd { fd: p.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
            nfds += 1;
        }
        if let Some(ref p) = stdout {
            stdout_idx = Some(nfds);
            pollfds[nfds] = libc::pollfd { fd: p.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            nfds += 1;
        }
        if let Some(ref p) = stderr {
            stderr_idx = Some(nfds);
            pollfds[nfds] = libc::pollfd { fd: p.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            nfds += 1;
        }

        let timeout_ms = remaining.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
        // SAFETY: pollfds holds nfds valid open pipe descriptors for the duration of the call.
        let rc = unsafe { libc::poll(pollfds.as_mut_ptr(), nfds as libc::nfds_t, timeout_ms) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if rc == 0 {
            continue;
        }

        if let Some(i) = stdin_idx {
            let rev = pollfds[i].revents;
            if rev != 0 {
                let mut close_stdin = (rev & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0;
                if !close_stdin
                    && let Some(data) = stdin_data
                    && let Some(ref mut pipe) = stdin
                {
                    loop {
                        if stdin_pos >= data.len() {
                            close_stdin = true;
                            break;
                        }
                        match pipe.write(&data[stdin_pos..]) {
                            Ok(0) => {
                                close_stdin = true;
                                break;
                            }
                            Ok(n) => {
                                stdin_pos += n;
                                if stdin_pos >= data.len() {
                                    close_stdin = true;
                                    break;
                                }
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                            Err(ref e) if e.kind() == io::ErrorKind::BrokenPipe => {
                                close_stdin = true;
                                break;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }
                if close_stdin {
                    stdin = None;
                }
            }
        }

        if let Some(i) = stdout_idx
            && pollfds[i].revents != 0
            && let Some(ref mut pipe) = stdout
        {
            let mut eof = false;
            loop {
                match pipe.read(&mut temp_chunk) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => match cap_mode {
                        OutputCapMode::FailOnOverrun => {
                            if stdout_buf.len() + n > MAX_COMMAND_OUTPUT_BYTES {
                                return Err(io::Error::new(
                                    io::ErrorKind::OutOfMemory,
                                    "Command stdout exceeded buffer cap",
                                ));
                            }
                            stdout_buf.extend_from_slice(&temp_chunk[..n]);
                        }
                        OutputCapMode::DrainExcess => {
                            let room = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(stdout_buf.len());
                            if room > 0 {
                                stdout_buf.extend_from_slice(&temp_chunk[..n.min(room)]);
                            }
                        }
                    },
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            if eof {
                stdout = None;
            }
        }

        if let Some(i) = stderr_idx
            && pollfds[i].revents != 0
            && let Some(ref mut pipe) = stderr
        {
            let mut eof = false;
            loop {
                match pipe.read(&mut temp_chunk) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        let slice = &temp_chunk[..n];
                        if let Some(cb) = on_stderr_line.as_deref_mut() {
                            for &b in slice {
                                if b == b'\n' {
                                    let line = String::from_utf8_lossy(&stderr_line_buf);
                                    cb(line.trim_end_matches('\r'));
                                    stderr_line_buf.clear();
                                } else if stderr_line_buf.len() < MAX_STREAM_LINE_BYTES {
                                    stderr_line_buf.push(b);
                                }
                            }
                        }
                        match cap_mode {
                            OutputCapMode::FailOnOverrun => {
                                if stderr_buf.len() + n > MAX_COMMAND_OUTPUT_BYTES {
                                    return Err(io::Error::new(
                                        io::ErrorKind::OutOfMemory,
                                        "Command stderr exceeded buffer cap",
                                    ));
                                }
                                stderr_buf.extend_from_slice(slice);
                            }
                            OutputCapMode::DrainExcess => {
                                let room = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(stderr_buf.len());
                                if room > 0 {
                                    stderr_buf.extend_from_slice(&slice[..n.min(room)]);
                                }
                            }
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            if eof {
                if !stderr_line_buf.is_empty()
                    && let Some(cb) = on_stderr_line.as_deref_mut()
                {
                    let line = String::from_utf8_lossy(&stderr_line_buf);
                    cb(line.trim_end_matches('\r'));
                    stderr_line_buf.clear();
                }
                stderr = None;
            }
        }
    }

    let exit_code = loop {
        if let Some(status) = guard.child.try_wait()? {
            break status.code().unwrap_or(-1);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Command exceeded monotonic deadline",
            ));
        }
        thread::sleep(Duration::from_millis(5).min(remaining));
    };

    guard.defuse();
    Ok((exit_code, stdout_buf, stderr_buf))
}

/// Represents an open directory file descriptor bound to a verified secure directory hierarchy
pub struct SecureDir {
    fd: RawFd,
}

impl Drop for SecureDir {
    fn drop(&mut self) {
        if self.fd >= 0 {
            // SAFETY: self.fd is exclusively owned by this SecureDir instance.
            unsafe {
                libc::close(self.fd);
            }
        }
    }
}

impl std::os::unix::io::AsRawFd for SecureDir {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

/// Trusted existing system/user base roots that may legitimately traverse symlinks on
/// immutable Linux distributions (e.g. OSTree `/home -> /var/home`) or user-relocated
/// XDG directories (`~/.cache`, `~/.config`, `~/.local`). Everything below the matched
/// root is still traversed strictly with `O_NOFOLLOW`.
fn trusted_base_roots() -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    for var in ["XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_RUNTIME_DIR", "HOME"] {
        if let Some(val) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            let p = std::path::PathBuf::from(val);
            if p.is_absolute() && !p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                if var == "HOME" {
                    roots.push(p.join(".config"));
                    roots.push(p.join(".cache"));
                    roots.push(p.join(".local"));
                    roots.push(p.join(".local/share"));
                }
                roots.push(p);
            }
        }
    }
    for sys in ["/home", "/var/home", "/run", "/dev/shm", "/tmp", "/var/tmp"] {
        roots.push(std::path::PathBuf::from(sys));
    }
    roots
}

fn split_trusted_prefix_from(abs_path: &Path, roots: &[std::path::PathBuf]) -> (Option<std::path::PathBuf>, std::path::PathBuf) {
    let mut best: Option<(usize, std::path::PathBuf, std::path::PathBuf)> = None;
    for root in roots {
        if let Ok(rel) = abs_path.strip_prefix(root) {
            let depth = root.components().count();
            if best.as_ref().is_none_or(|(best_depth, ..)| depth > *best_depth)
                && let Ok(canon) = fs::canonicalize(root)
            {
                best = Some((depth, canon, rel.to_path_buf()));
            }
        }
    }
    match best {
        Some((_, canon, rel)) => (Some(canon), rel),
        None => (None, abs_path.to_path_buf()),
    }
}

fn step_dir_component(cur_fd: RawFd, name: &std::ffi::OsStr, current_uid: libc::uid_t, allow_create: bool) -> io::Result<RawFd> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_str = CString::new(name.as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    // SAFETY: cur_fd is an open directory FD and c_str is NUL-terminated.
    let mut next_fd = unsafe {
        libc::openat(
            cur_fd,
            c_str.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };

    let mut created_now = false;
    if next_fd < 0 && allow_create {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ENOENT) {
            // SAFETY: cur_fd is an open directory FD and c_str is NUL-terminated.
            let ret = unsafe { libc::mkdirat(cur_fd, c_str.as_ptr(), 0o700) };
            if ret != 0 {
                let mkdir_err = io::Error::last_os_error();
                if mkdir_err.raw_os_error() != Some(libc::EEXIST) {
                    return Err(mkdir_err);
                }
            } else {
                created_now = true;
            }

            // SAFETY: cur_fd is an open directory FD and c_str is NUL-terminated.
            next_fd = unsafe {
                libc::openat(
                    cur_fd,
                    c_str.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
        }
    }

    if next_fd < 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: stat is plain old data; fstat initializes it for the valid descriptor next_fd.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: next_fd is an open descriptor and stat is a valid mutable pointer.
    if unsafe { libc::fstat(next_fd, &mut stat) } != 0 {
        let err = io::Error::last_os_error();
        // SAFETY: next_fd is an open descriptor owned by this function.
        unsafe { libc::close(next_fd) };
        return Err(err);
    }

    // Must be a directory (S_IFDIR) and definitely not a symlink
    if (stat.st_mode & libc::S_IFMT) != libc::S_IFDIR {
        // SAFETY: next_fd is an open descriptor owned by this function.
        unsafe { libc::close(next_fd) };
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Path component is not a directory or is a symbolic link",
        ));
    }

    // Reject any component owned by a foreign non-root user
    if stat.st_uid != current_uid && stat.st_uid != 0 {
        // SAFETY: next_fd is an open descriptor owned by this function.
        unsafe { libc::close(next_fd) };
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Path component is owned by a foreign user",
        ));
    }

    // Enforce strict 0700 only on directories this call created. Pre-existing
    // ancestors ($HOME, ~/Pictures, /dev/shm, ...) belong to the user and must
    // keep their permissions; chmod-ing them broke shared and served folders.
    if created_now && stat.st_uid == current_uid && (stat.st_mode & 0o777) != 0o700 {
        // SAFETY: next_fd is an open directory owned by current_uid.
        let _ = unsafe { libc::fchmod(next_fd, 0o700) };
    }

    Ok(next_fd)
}

impl SecureDir {
    /// Opens or creates each component of a directory hierarchy without following symlinks
    /// (`O_NOFOLLOW`) inside the application subtree, while resolving trusted existing
    /// system/XDG base prefixes (supporting OSTree `/home -> /var/home` and symlinked XDG roots).
    /// Retains and returns an open directory descriptor bound to the final directory.
    pub fn open_or_create_hierarchy(path: &Path) -> io::Result<Self> {
        let roots = trusted_base_roots();
        Self::open_or_create_with_roots(path, &roots)
    }

    fn open_or_create_with_roots(path: &Path, roots: &[std::path::PathBuf]) -> io::Result<Self> {
        if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Parent directory traversal (..) is not permitted in secure paths",
            ));
        }

        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        if abs_path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Parent directory traversal (..) is not permitted in secure paths",
            ));
        }

        // SAFETY: literal c"/" is a valid NUL-terminated path.
        let mut cur_fd = unsafe {
            libc::open(
                c"/".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if cur_fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let current_uid = current_uid();
        let (trusted_prefix, suffix) = split_trusted_prefix_from(&abs_path, roots);

        if let Some(prefix) = trusted_prefix {
            for comp in prefix.components() {
                if let std::path::Component::Normal(c_name) = comp {
                    match step_dir_component(cur_fd, c_name, current_uid, false) {
                        Ok(next_fd) => {
                            // SAFETY: cur_fd is an open descriptor owned by this function.
                            unsafe { libc::close(cur_fd) };
                            cur_fd = next_fd;
                        }
                        Err(err) => {
                            // SAFETY: cur_fd is an open descriptor owned by this function.
                            unsafe { libc::close(cur_fd) };
                            return Err(err);
                        }
                    }
                }
            }
        }

        for comp in suffix.components() {
            match comp {
                std::path::Component::RootDir | std::path::Component::Prefix(_) | std::path::Component::CurDir => continue,
                std::path::Component::ParentDir => {
                    // SAFETY: cur_fd is an open descriptor owned by this function.
                    unsafe { libc::close(cur_fd) };
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Parent directory traversal (..) is not permitted in secure paths",
                    ));
                }
                std::path::Component::Normal(c_name) => {
                    match step_dir_component(cur_fd, c_name, current_uid, true) {
                        Ok(next_fd) => {
                            // SAFETY: cur_fd is an open descriptor owned by this function.
                            unsafe { libc::close(cur_fd) };
                            cur_fd = next_fd;
                        }
                        Err(err) => {
                            // SAFETY: cur_fd is an open descriptor owned by this function.
                            unsafe { libc::close(cur_fd) };
                            return Err(err);
                        }
                    }
                }
            }
        }

        Ok(Self { fd: cur_fd })
    }

    /// Opens (creating if needed) a directory the *user* chose, such as an export
    /// folder or the folder holding their photos. Unlike `open_or_create_hierarchy`
    /// this follows symlinks and accepts directories owned by other uids (NAS / USB
    /// mounts), and never changes permissions. Files are still staged with
    /// O_EXCL | O_NOFOLLOW and renamed into place relative to the opened descriptor.
    pub fn open_user_dir(path: &Path) -> io::Result<Self> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Parent directory traversal (..) is not permitted in output paths",
            ));
        }
        fs::create_dir_all(path)?;
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        // SAFETY: c_path is NUL-terminated; O_DIRECTORY ensures only a directory FD is returned.
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd })
    }

    /// Creates an exclusive, no-follow temporary staging file with mode 0600 relative to this directory
    pub fn open_staging_file(&self, prefix: &str, suffix: &str) -> io::Result<(fs::File, std::ffi::CString)> {
        self.open_staging_file_with_mode(prefix, suffix, 0o600)
    }

    /// Creates an exclusive, no-follow temporary staging file with the requested mode (masked by process umask)
    /// relative to this directory.
    pub fn open_staging_file_with_mode(
        &self,
        prefix: &str,
        suffix: &str,
        mode: libc::mode_t,
    ) -> io::Result<(fs::File, std::ffi::CString)> {
        use std::ffi::CString;
        use std::os::unix::io::FromRawFd;

        let temp_name = format!(
            "{}_{}_{}_{}",
            prefix,
            std::process::id(),
            STAGING_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            suffix
        );
        let c_name = CString::new(temp_name.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        // SAFETY: self.fd is an open directory FD and c_name is NUL-terminated.
        let fd = unsafe {
            libc::openat(
                self.fd,
                c_name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                mode,
            )
        };

        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: fd was just opened exclusively and is owned solely by this File.
        let file = unsafe { fs::File::from_raw_fd(fd) };
        Ok((file, c_name))
    }

    /// Opens an existing file read-only relative to this directory without following symlinks
    pub fn open_existing_file_ro(&self, c_name: &std::ffi::CStr) -> io::Result<fs::File> {
        use std::os::unix::io::FromRawFd;

        // SAFETY: self.fd is an open directory FD and c_name is a valid CStr.
        let fd = unsafe {
            libc::openat(
                self.fd,
                c_name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd was just opened and is owned solely by the returned File.
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }

    /// Unlinks a file relative to this directory
    pub fn unlink_file(&self, c_name: &std::ffi::CStr) -> io::Result<()> {
        // SAFETY: self.fd is an open directory FD and c_name is a valid CStr.
        let ret = unsafe { libc::unlinkat(self.fd, c_name.as_ptr(), 0) };
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Atomically renames a file relative to this directory
    pub fn rename_file(&self, from_name: &std::ffi::CStr, to_name: &std::ffi::CStr) -> io::Result<()> {
        // SAFETY: self.fd is an open directory FD and both names are valid CStrs.
        let ret = unsafe {
            libc::renameat(self.fd, from_name.as_ptr(), self.fd, to_name.as_ptr())
        };
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// RAII Guard ensuring staging files created relative to a SecureDir are unlinked on error or timeout
pub struct SecureDirCleanupGuard<'a> {
    pub secure_dir: &'a SecureDir,
    pub filename: std::ffi::CString,
    pub active: bool,
}

impl<'a> Drop for SecureDirCleanupGuard<'a> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.secure_dir.unlink_file(&self.filename);
        }
    }
}

/// Ensures directory exists with strict Mode 0700 permissions without following symbolic links
pub fn ensure_secure_dir(path: &Path) -> io::Result<()> {
    let dir = SecureDir::open_or_create_hierarchy(path)?;
    // The leaf is an engine-owned private directory: tighten it to 0700 even if it
    // pre-existed with looser permissions (ancestors are left untouched).
    // SAFETY: libc::stat is plain old data; fstat initializes it for valid dir.fd.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: dir.fd is an open directory descriptor and stat is a valid mutable pointer.
    if unsafe { libc::fstat(dir.fd, &mut stat) } == 0
        && stat.st_uid == current_uid()
        && (stat.st_mode & 0o777) != 0o700
    {
        // SAFETY: dir.fd is an open directory owned by the current user.
        let _ = unsafe { libc::fchmod(dir.fd, 0o700) };
    }
    Ok(())
}

/// Verifies that a file is a regular file and NOT a symlink
pub fn verify_safe_file(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Symlink access is strictly prohibited",
        ));
    }
    if !meta.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Target is not a regular file",
        ));
    }
    Ok(())
}

/// Reads a file securely, verifying it is a regular file, rejecting symlinks via O_NOFOLLOW and descriptor verification,
/// and enforcing a hard byte limit (take(max + 1)) to prevent memory exhaustion / DoS.
pub fn read_secure_file(path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::{AsRawFd, FromRawFd};

    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    // SAFETY: c_path is NUL-terminated; O_NOFOLLOW prevents following symlinks.
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd was just opened and is owned exclusively by file.
    let file = unsafe { fs::File::from_raw_fd(fd) };

    // SAFETY: libc::stat is plain old data; fstat initializes it for the open file descriptor.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: file.as_raw_fd() is valid and stat is a valid mutable pointer.
    if unsafe { libc::fstat(file.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }

    if (stat.st_mode & libc::S_IFMT) != libc::S_IFREG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Target is not a regular file",
        ));
    }

    let mut buf = Vec::new();
    file.take((max_bytes as u64) + 1).read_to_end(&mut buf)?;
    if buf.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!("File exceeded maximum allowed size of {} bytes", max_bytes),
        ));
    }
    Ok(buf)
}

/// Atomically writes sensitive data to a file with Mode 0600 permissions using descriptor-bound staging
pub fn atomic_write_secure(target: &Path, data: &[u8]) -> io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let secure_dir = SecureDir::open_or_create_hierarchy(parent)?;
    atomic_write_parts_in(&secure_dir, target, &[data], 0o600)
}

/// Atomic write into a user-chosen folder (exports, sidecars, render output) with
/// umask-respecting mode 0644. The folder may be a symlink or live on a mount owned by
/// another uid; see [`SecureDir::open_user_dir`]. Engine-private state keeps using
/// [`atomic_write_secure`] (strict 0600).
pub fn atomic_write_user(target: &Path, data: &[u8]) -> io::Result<()> {
    atomic_write_user_parts(target, &[data])
}

/// [`atomic_write_user`] for a file made of several buffers written in order, so a
/// large export need not be copied into one buffer first.
pub fn atomic_write_user_parts(target: &Path, parts: &[&[u8]]) -> io::Result<()> {
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let dir = SecureDir::open_user_dir(parent)?;
    atomic_write_parts_in(&dir, target, parts, 0o644)
}

fn atomic_write_parts_in(
    secure_dir: &SecureDir,
    target: &Path,
    parts: &[&[u8]],
    mode: libc::mode_t,
) -> io::Result<()> {
    use std::ffi::CString;

    let filename = target
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid target filename"))?;

    let c_dest_name = CString::new(filename.as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    let (mut stage_file, c_stage_name) = secure_dir.open_staging_file_with_mode(".tmp_atomic", filename, mode)?;
    let mut guard = SecureDirCleanupGuard {
        secure_dir,
        filename: c_stage_name.clone(),
        active: true,
    };

    for part in parts {
        stage_file.write_all(part)?;
    }
    stage_file.sync_all()?;

    secure_dir.rename_file(&c_stage_name, &c_dest_name)?;
    guard.active = false;
    Ok(())
}

/// Streams child process stdout directly to a file descriptor, enforcing a hard byte limit and deadline.
/// Uses `libc::poll()` to drain both stdout and stderr to EOF before reaping the child and defusing the
/// process-group guard; if `max_bytes` is exceeded or timeout occurs, the process group is reaped.
pub fn run_bounded_command_stream_to_file(
    mut cmd: Command,
    target_file: &mut fs::File,
    timeout: Duration,
    max_bytes: usize,
) -> io::Result<usize> {
    use std::os::unix::io::AsRawFd;

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    let guard = ProcessGroupGuard::new(&mut child);
    let deadline = Instant::now() + timeout;

    let mut stdout = guard.child.stdout.take();
    let mut stderr = guard.child.stderr.take();

    if let Some(ref p) = stdout {
        set_nonblocking(p.as_raw_fd());
    }
    if let Some(ref p) = stderr {
        set_nonblocking(p.as_raw_fd());
    }

    let mut total_written = 0usize;
    let mut chunk = [0u8; 8192];
    let mut err_chunk = [0u8; 4096];
    let mut stderr_buf = Vec::new();

    while stdout.is_some() || stderr.is_some() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Command exceeded monotonic deadline",
            ));
        }

        let mut pollfds = [libc::pollfd { fd: -1, events: 0, revents: 0 }; 2];
        let mut nfds = 0usize;
        let mut stdout_idx = None;
        let mut stderr_idx = None;

        if let Some(ref p) = stdout {
            stdout_idx = Some(nfds);
            pollfds[nfds] = libc::pollfd { fd: p.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            nfds += 1;
        }
        if let Some(ref p) = stderr {
            stderr_idx = Some(nfds);
            pollfds[nfds] = libc::pollfd { fd: p.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            nfds += 1;
        }

        let timeout_ms = remaining.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
        // SAFETY: pollfds holds nfds valid open pipe descriptors for the duration of the call.
        let rc = unsafe { libc::poll(pollfds.as_mut_ptr(), nfds as libc::nfds_t, timeout_ms) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if rc == 0 {
            continue;
        }

        if let Some(i) = stdout_idx
            && pollfds[i].revents != 0
            && let Some(ref mut pipe) = stdout
        {
            let mut eof = false;
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        if total_written.saturating_add(n) > max_bytes {
                            return Err(io::Error::new(
                                io::ErrorKind::OutOfMemory,
                                format!("Output exceeded maximum allowed size of {} bytes", max_bytes),
                            ));
                        }
                        target_file.write_all(&chunk[..n])?;
                        total_written += n;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            if eof {
                stdout = None;
            }
        }

        if let Some(i) = stderr_idx
            && pollfds[i].revents != 0
            && let Some(ref mut pipe) = stderr
        {
            let mut eof = false;
            loop {
                match pipe.read(&mut err_chunk) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        // Drain all stderr to EOF while keeping captured diagnostics bounded.
                        let room = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(stderr_buf.len());
                        if room > 0 {
                            stderr_buf.extend_from_slice(&err_chunk[..n.min(room)]);
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            if eof {
                stderr = None;
            }
        }
    }

    let exit_code = loop {
        if let Some(status) = guard.child.try_wait()? {
            break status.code().unwrap_or(-1);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Command exceeded monotonic deadline",
            ));
        }
        thread::sleep(Duration::from_millis(5).min(remaining));
    };

    if exit_code != 0 {
        return Err(io::Error::other(format!(
            "Command failed with exit code {}: {}",
            exit_code,
            String::from_utf8_lossy(&stderr_buf).trim()
        )));
    }

    target_file.sync_all()?;
    guard.defuse();
    Ok(total_written)
}

/// Verifies an open file descriptor is a regular file, owned by current user, mode 0600, and within size limit
pub fn verify_secure_open_file(file: &fs::File, max_bytes: usize) -> io::Result<u64> {
    use std::os::unix::io::AsRawFd;

    // SAFETY: libc::stat is plain old data; fstat initializes it for the open file descriptor.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: file.as_raw_fd() is valid and stat is a valid mutable pointer.
    if unsafe { libc::fstat(file.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }

    // Must be regular file (S_IFREG)
    if (stat.st_mode & libc::S_IFMT) != libc::S_IFREG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Target file is not a regular file",
        ));
    }

    // Must be owned by current user
    if stat.st_uid != current_uid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Target file is not owned by current user",
        ));
    }

    // Must be private (no group or others permissions: st_mode & 0o077 == 0)
    if (stat.st_mode & 0o077) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Target file permissions are not private (mode 0600 required)",
        ));
    }

    let size = stat.st_size;
    if size <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Target file is empty",
        ));
    }
    if (size as u64) > (max_bytes as u64) {
        return Err(io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!("File size {} exceeds maximum allowed limit {}", size, max_bytes),
        ));
    }

    Ok(size as u64)
}

/// Returns a secure user preview directory with strict 0700 permissions and UID ownership verification.
/// Prefers /dev/shm/omastudio-<uid> if safe, falling back to ~/.cache/omastudio.
pub fn get_secure_preview_dir() -> std::path::PathBuf {
    use std::os::unix::fs::MetadataExt;
    let uid = current_uid();
    let shm_dir = Path::new("/dev/shm");
    if shm_dir.exists() && shm_dir.is_dir() {
        let user_shm = std::path::PathBuf::from(format!("/dev/shm/omastudio-{}", uid));
        if ensure_secure_dir(&user_shm).is_ok()
            && let Ok(meta) = fs::symlink_metadata(&user_shm)
            && meta.file_type().is_dir() && !meta.file_type().is_symlink() && meta.uid() == uid
        {
            return user_shm;
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let cache_dir = std::path::PathBuf::from(home).join(".cache/omastudio");
    let _ = ensure_secure_dir(&cache_dir);
    cache_dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_dir_allows_symlinked_trusted_root_but_rejects_app_subtree_symlink() {
        let base = std::path::PathBuf::from(format!("/tmp/omastudio_secdir_prefix_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let real_xdg = base.join("real_cache");
        let link_xdg = base.join("link_cache");
        fs::create_dir_all(&real_xdg).unwrap();
        std::os::unix::fs::symlink(&real_xdg, &link_xdg).unwrap();

        let target = link_xdg.join("omastudio/cloud");
        let (prefix, suffix) = split_trusted_prefix_from(&target, &[link_xdg.clone(), std::path::PathBuf::from("/tmp")]);
        assert_eq!(prefix.as_deref(), Some(fs::canonicalize(&real_xdg).unwrap().as_path()));
        assert_eq!(suffix, Path::new("omastudio/cloud"));

        let opened = SecureDir::open_or_create_with_roots(&target, &[link_xdg.clone(), std::path::PathBuf::from("/tmp")])
            .expect("symlinked trusted XDG base prefix must succeed");
        drop(opened);
        assert!(real_xdg.join("omastudio/cloud").is_dir());

        // A symlink inside the application subtree below the trusted root must still be rejected.
        let evil_real = base.join("evil_real");
        fs::create_dir_all(&evil_real).unwrap();
        let evil_link = real_xdg.join("omastudio/evil_link");
        std::os::unix::fs::symlink(&evil_real, &evil_link).unwrap();
        let evil_target = link_xdg.join("omastudio/evil_link/sub");
        assert!(
            SecureDir::open_or_create_with_roots(&evil_target, &[link_xdg.clone(), std::path::PathBuf::from("/tmp")]).is_err(),
            "symlink inside application subtree must still be rejected by O_NOFOLLOW"
        );

        let _ = fs::remove_dir_all(&base);
    }
}

