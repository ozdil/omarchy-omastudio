use std::fs::{self};
use std::io::{self, Read, Write};
use std::os::unix::io::RawFd;
use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

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

/// Runs a command with a monotonic deadline, non-blocking I/O polling, and buffer limit
pub fn run_bounded_command(
    mut cmd: Command,
    timeout: Duration,
) -> io::Result<(i32, Vec<u8>, Vec<u8>)> {
    use std::os::unix::io::AsRawFd;

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    let guard = ProcessGroupGuard::new(&mut child);

    let deadline = Instant::now() + timeout;

    let stdout_fd = guard.child.stdout.as_ref().map(|p| p.as_raw_fd());
    let stderr_fd = guard.child.stderr.as_ref().map(|p| p.as_raw_fd());

    // Set non-blocking on pipes
    for &fd_opt in &[stdout_fd, stderr_fd] {
        if let Some(fd) = fd_opt {
            // SAFETY: fd is valid and owned by child process stdio
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                if flags >= 0 {
                    libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
            }
        }
    }

    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let mut temp_chunk = [0u8; 4096];

    let mut child_exited = false;
    let mut exit_code = -1;

    while Instant::now() < deadline {
        // Poll for child exit
        if !child_exited {
            if let Ok(Some(status)) = guard.child.try_wait() {
                child_exited = true;
                exit_code = status.code().unwrap_or(-1);
            }
        }

        // Drain stdout
        if let Some(ref mut pipe) = guard.child.stdout {
            loop {
                match pipe.read(&mut temp_chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        if stdout_buf.len() + n > MAX_COMMAND_OUTPUT_BYTES {
                            return Err(io::Error::new(
                                io::ErrorKind::OutOfMemory,
                                "Command stdout exceeded buffer cap",
                            ));
                        }
                        stdout_buf.extend_from_slice(&temp_chunk[..n]);
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e),
                }
            }
        }

        // Drain stderr
        if let Some(ref mut pipe) = guard.child.stderr {
            loop {
                match pipe.read(&mut temp_chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        if stderr_buf.len() + n > MAX_COMMAND_OUTPUT_BYTES {
                            return Err(io::Error::new(
                                io::ErrorKind::OutOfMemory,
                                "Command stderr exceeded buffer cap",
                            ));
                        }
                        stderr_buf.extend_from_slice(&temp_chunk[..n]);
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e),
                }
            }
        }

        if child_exited {
            // Drain any remaining buffered data from stdout
            if let Some(ref mut pipe) = guard.child.stdout {
                loop {
                    match pipe.read(&mut temp_chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            if stdout_buf.len() + n > MAX_COMMAND_OUTPUT_BYTES {
                                return Err(io::Error::new(
                                    io::ErrorKind::OutOfMemory,
                                    "Command stdout exceeded buffer cap",
                                ));
                            }
                            stdout_buf.extend_from_slice(&temp_chunk[..n]);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }
            }

            // Drain any remaining buffered data from stderr
            if let Some(ref mut pipe) = guard.child.stderr {
                loop {
                    match pipe.read(&mut temp_chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            if stderr_buf.len() + n > MAX_COMMAND_OUTPUT_BYTES {
                                return Err(io::Error::new(
                                    io::ErrorKind::OutOfMemory,
                                    "Command stderr exceeded buffer cap",
                                ));
                            }
                            stderr_buf.extend_from_slice(&temp_chunk[..n]);
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }
            }
            break;
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        let sleep_step = Duration::from_millis(50).min(remaining);
        if sleep_step.is_zero() {
            break;
        }
        thread::sleep(sleep_step);
    }

    if !child_exited {
        // Timed out: guard will reap the process group
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Command exceeded monotonic deadline",
        ));
    }

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

impl SecureDir {
    /// Opens or creates each component of a directory hierarchy without following symlinks (O_NOFOLLOW).
    /// Retains and returns an open directory descriptor bound to the final directory.
    pub fn open_or_create_hierarchy(path: &Path) -> io::Result<Self> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };

        // Open root directory descriptor
        let mut cur_fd = unsafe {
            libc::open(
                c"/".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if cur_fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let current_uid = unsafe { libc::getuid() };

        for comp in abs_path.components() {
            match comp {
                std::path::Component::RootDir | std::path::Component::Prefix(_) => continue,
                std::path::Component::CurDir => continue,
                std::path::Component::ParentDir => {
                    unsafe { libc::close(cur_fd); }
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Parent directory traversal (..) is not permitted in secure paths",
                    ));
                }
                std::path::Component::Normal(c_name) => {
                    let c_str = CString::new(c_name.as_bytes()).map_err(|e| {
                        unsafe { libc::close(cur_fd); }
                        io::Error::new(io::ErrorKind::InvalidInput, e)
                    })?;

                    // Attempt to open directory without following symlinks
                    let mut next_fd = unsafe {
                        libc::openat(
                            cur_fd,
                            c_str.as_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        )
                    };

                    if next_fd < 0 {
                        let err = io::Error::last_os_error();
                        if err.raw_os_error() == Some(libc::ENOENT) {
                            // Directory does not exist: create with mode 0700
                            let ret = unsafe { libc::mkdirat(cur_fd, c_str.as_ptr(), 0o700) };
                            if ret != 0 {
                                let mkdir_err = io::Error::last_os_error();
                                if mkdir_err.raw_os_error() != Some(libc::EEXIST) {
                                    unsafe { libc::close(cur_fd); }
                                    return Err(mkdir_err);
                                }
                            }

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
                        let err = io::Error::last_os_error();
                        unsafe { libc::close(cur_fd); }
                        return Err(err);
                    }

                    // Verify opened descriptor with fstat
                    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
                    if unsafe { libc::fstat(next_fd, &mut stat) } != 0 {
                        let err = io::Error::last_os_error();
                        unsafe {
                            libc::close(next_fd);
                            libc::close(cur_fd);
                        }
                        return Err(err);
                    }

                    // Must be a directory (S_IFDIR) and definitely not a symlink
                    if (stat.st_mode & libc::S_IFMT) != libc::S_IFDIR {
                        unsafe {
                            libc::close(next_fd);
                            libc::close(cur_fd);
                        }
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "Path component is not a directory or is a symbolic link",
                        ));
                    }

                    // If owned by current user, enforce strict 0700 permissions descriptor-bound
                    if stat.st_uid == current_uid && (stat.st_mode & 0o777) != 0o700 {
                        let _ = unsafe { libc::fchmod(next_fd, 0o700) };
                    }

                    unsafe { libc::close(cur_fd); }
                    cur_fd = next_fd;
                }
            }
        }

        Ok(Self { fd: cur_fd })
    }

    /// Creates an exclusive, no-follow temporary staging file with mode 0600 relative to this directory
    pub fn open_staging_file(&self, prefix: &str, suffix: &str) -> io::Result<(fs::File, std::ffi::CString)> {
        use std::ffi::CString;
        use std::os::unix::io::FromRawFd;

        let temp_name = format!(
            "{}_{}_{}_{}",
            prefix,
            std::process::id(),
            Instant::now().elapsed().as_nanos(),
            suffix
        );
        let c_name = CString::new(temp_name.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let fd = unsafe {
            libc::openat(
                self.fd,
                c_name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };

        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let file = unsafe { fs::File::from_raw_fd(fd) };
        Ok((file, c_name))
    }

    /// Opens an existing file read-only relative to this directory without following symlinks
    pub fn open_existing_file_ro(&self, c_name: &std::ffi::CStr) -> io::Result<fs::File> {
        use std::os::unix::io::FromRawFd;

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
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }

    /// Unlinks a file relative to this directory
    pub fn unlink_file(&self, c_name: &std::ffi::CStr) -> io::Result<()> {
        let ret = unsafe { libc::unlinkat(self.fd, c_name.as_ptr(), 0) };
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Atomically renames a file relative to this directory
    pub fn rename_file(&self, from_name: &std::ffi::CStr, to_name: &std::ffi::CStr) -> io::Result<()> {
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
    let _dir = SecureDir::open_or_create_hierarchy(path)?;
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

/// Atomically writes sensitive data to a file with Mode 0600 permissions using descriptor-bound staging
pub fn atomic_write_secure(target: &Path, data: &[u8]) -> io::Result<()> {
    use std::ffi::CString;

    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let secure_dir = SecureDir::open_or_create_hierarchy(parent)?;

    let filename = target
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid target filename"))?;

    let c_dest_name = CString::new(filename.as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    let (mut stage_file, c_stage_name) = secure_dir.open_staging_file(".tmp_atomic", filename)?;
    let mut guard = SecureDirCleanupGuard {
        secure_dir: &secure_dir,
        filename: c_stage_name.clone(),
        active: true,
    };

    stage_file.write_all(data)?;
    stage_file.sync_all()?;

    secure_dir.rename_file(&c_stage_name, &c_dest_name)?;
    guard.active = false;
    Ok(())
}

/// Streams child process stdout directly to a file descriptor, enforcing a hard byte limit and deadline.
/// If max_bytes is exceeded or timeout occurs, the process group is reaped and an error is returned.
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

    let stdout_fd = guard.child.stdout.as_ref().map(|p| p.as_raw_fd());
    let stderr_fd = guard.child.stderr.as_ref().map(|p| p.as_raw_fd());

    // Set non-blocking on pipes
    for &fd_opt in &[stdout_fd, stderr_fd] {
        if let Some(fd) = fd_opt {
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                if flags >= 0 {
                    libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
            }
        }
    }

    let mut total_written = 0usize;
    let mut chunk = [0u8; 8192];
    let mut err_chunk = [0u8; 1024];
    let mut stderr_buf = Vec::new();
    let mut child_exited = false;
    let mut exit_code = -1;
    let mut stdout_closed = false;

    while Instant::now() < deadline {
        if !child_exited {
            if let Ok(Some(status)) = guard.child.try_wait() {
                child_exited = true;
                exit_code = status.code().unwrap_or(-1);
            }
        }

        // Drain stdout into target_file
        if let Some(ref mut pipe) = guard.child.stdout {
            if !stdout_closed {
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) => {
                            stdout_closed = true;
                            break;
                        }
                        Ok(n) => {
                            if total_written + n > max_bytes {
                                return Err(io::Error::new(
                                    io::ErrorKind::OutOfMemory,
                                    format!("Output exceeded maximum allowed size of {} bytes", max_bytes),
                                ));
                            }
                            target_file.write_all(&chunk[..n])?;
                            total_written += n;
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }
            }
        }

        // Drain stderr
        if let Some(ref mut pipe) = guard.child.stderr {
            loop {
                match pipe.read(&mut err_chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        if stderr_buf.len() + n <= MAX_COMMAND_OUTPUT_BYTES {
                            stderr_buf.extend_from_slice(&err_chunk[..n]);
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e),
                }
            }
        }

        if child_exited && stdout_closed {
            break;
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        let sleep_step = Duration::from_millis(10).min(remaining);
        if sleep_step.is_zero() {
            break;
        }
        thread::sleep(sleep_step);
    }

    if !child_exited {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Command exceeded monotonic deadline",
        ));
    }

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

    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
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
    let current_uid = unsafe { libc::getuid() };
    if stat.st_uid != current_uid {
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

