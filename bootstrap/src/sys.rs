// The process and clock seam, the counterpart to [`crate::vfs`]. A shell, a process
// id, a wall clock, and a blocking sleep are all things a browser tab does not have,
// so the crate asks for them here and the wasm build answers honestly instead of
// trapping. See docs/frontends/wasm.md#the-seams.

use std::path::Path;
use std::time::Duration;

// What the crate reads off a finished command. Narrower than `std::process::Output`:
// the exit code as a number, and the two streams.
pub struct ShellOutput {
    pub success: bool,
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    // The command ran past the command timeout and was killed; `code` is 124.
    pub timed_out: bool,
}

// The most of each stream a command keeps, the tail: what a reader of the evidence
// or a session's output tail can use. A chatty command never fills memory.
pub const SHELL_CAPTURE_BYTES: usize = 4 * 1024 * 1024;

// The message a caught panic carried, for a panic boundary that reports instead of
// unwinding further (a job worker, an agent turn).
pub fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

impl ShellOutput {
    // The `{}` of a `std::process::ExitStatus`, kept so error text reads the same.
    pub fn status(&self) -> String {
        if self.success {
            "exit status: 0".to_string()
        } else {
            format!("exit status: {}", self.code)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::{Duration, Path, ShellOutput};

    // Run a command under the command timeout (limits::command_timeout): stdin closed,
    // both streams drained as they arrive and kept to their tails, the child killed
    // when the deadline passes and reported as exit 124.
    // Mirrors docs/consumers/gen.md#a-test-that-could-not-run-says-nothing.
    pub fn run_shell(script: &str, cwd: &Path) -> std::io::Result<ShellOutput> {
        run_shell_within(script, cwd, crate::limits::command_timeout())
    }

    pub fn run_shell_within(
        script: &str,
        cwd: &Path,
        timeout: Duration,
    ) -> std::io::Result<ShellOutput> {
        if crate::vfs::is_scoped() { return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "hosted workspaces run commands through delivery CI")); }
        use std::io::Read;
        use std::process::{Command, Stdio};
        use std::sync::{Arc, Mutex};
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(script)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Its own process group, so the timeout reaches what the script started
        // (a test runner's workers, a `sleep` in a pipeline), not only the shell.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn()?;
        // A drain keeps reading until the writer closes the pipe; the buffer holds the
        // tail, shared so a killed command still yields what it printed.
        fn drain<R: Read + Send + 'static>(mut pipe: R) -> Arc<Mutex<Vec<u8>>> {
            let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
            let shared = Arc::clone(&buf);
            std::thread::spawn(move || {
                let mut chunk = [0u8; 8192];
                loop {
                    let n = match pipe.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let mut b = shared.lock().unwrap_or_else(|p| p.into_inner());
                    b.extend_from_slice(&chunk[..n]);
                    if b.len() > super::SHELL_CAPTURE_BYTES {
                        let excess = b.len() - super::SHELL_CAPTURE_BYTES;
                        b.drain(..excess);
                    }
                }
            });
            buf
        }
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        let started = std::time::Instant::now();
        let status = loop {
            if let Some(st) = child.try_wait()? {
                break Some(st);
            }
            if started.elapsed() >= timeout {
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let (success, code, timed_out) = match status {
            Some(st) => (st.success(), st.code().unwrap_or(-1), false),
            None => {
                kill_group(&mut child);
                child.wait().ok();
                (false, 124, true)
            }
        };
        // Give the drains a moment to see the pipes close; a grandchild holding a pipe
        // open past the kill costs the tail of its output, never the build.
        let take = |buf: Option<Arc<Mutex<Vec<u8>>>>| -> Vec<u8> {
            let Some(buf) = buf else { return Vec::new() };
            let until = std::time::Instant::now() + Duration::from_secs(2);
            while Arc::strong_count(&buf) > 1 && std::time::Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
            let b = buf.lock().unwrap_or_else(|p| p.into_inner());
            b.clone()
        };
        let stdout = take(stdout);
        let mut stderr = take(stderr);
        if timed_out {
            if !stderr.is_empty() && !stderr.ends_with(b"\n") {
                stderr.push(b'\n');
            }
            stderr.extend_from_slice(
                format!(
                    "jazyk: command killed after {}s (JAZYK_COMMAND_TIMEOUT)\n",
                    timeout.as_secs()
                )
                .as_bytes(),
            );
        }
        Ok(ShellOutput {
            success,
            code,
            stdout,
            stderr,
            timed_out,
        })
    }

    // Kill a child spawned as its own process group leader, and every process in the
    // group. Elsewhere the child alone.
    fn kill_group(child: &mut std::process::Child) {
        #[cfg(unix)]
        {
            if let Ok(pid) = libc::pid_t::try_from(child.id()) {
                // SAFETY: kill(2) with a negative pid signals the group; it touches
                // no memory of this process.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
        }
        child.kill().ok();
    }

    pub fn process_id() -> u32 {
        if crate::vfs::is_scoped() { 1 } else { std::process::id() }
    }

    // Seconds since the epoch.
    pub fn now_unix() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default()
    }

    // Milliseconds since an arbitrary origin, for measuring one span.
    pub fn now_millis() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or_default()
    }

    pub fn sleep(d: Duration) {
        if crate::vfs::is_scoped() {
            let started = std::time::Instant::now();
            while started.elapsed() < d && !crate::hosted::cancelled() {
                std::thread::sleep((d-started.elapsed().min(d)).min(Duration::from_millis(100)));
            }
            return;
        }
        std::thread::sleep(d);
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::{Duration, Path, ShellOutput};

    // No shell in a tab. The callers all turn this into a message a person reads,
    // so it says what is missing rather than pretending the command failed.
    pub fn run_shell(script: &str, _cwd: &Path) -> std::io::Result<ShellOutput> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("the browser backend has no shell to run `{}` in", script),
        ))
    }

    pub fn run_shell_within(
        script: &str,
        cwd: &Path,
        _timeout: Duration,
    ) -> std::io::Result<ShellOutput> {
        run_shell(script, cwd)
    }

    // One backend per tab, and nothing to disambiguate against.
    pub fn process_id() -> u32 {
        1
    }

    pub fn now_unix() -> u64 {
        (js_sys::Date::now() / 1000.0) as u64
    }

    pub fn now_millis() -> f64 {
        js_sys::Date::now()
    }

    // The single thread the backend runs on is the one the page draws with, so
    // there is nothing to block: a sleep here would freeze the tab. Callers use it
    // for retry backoff, and a browser retry is immediate.
    pub fn sleep(_d: Duration) {}
}

// A monotonic instant. Natively `std::time::Instant`; in a tab the page's clock in
// milliseconds, which `std::time::Instant` cannot answer at all there. Carries only
// what the crate does with one: take it, subtract two, add a duration, compare.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub struct Instant(
    #[cfg(not(target_arch = "wasm32"))] std::time::Instant,
    #[cfg(target_arch = "wasm32")] f64,
);

impl Instant {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn now() -> Instant {
        Instant(std::time::Instant::now())
    }

    #[cfg(target_arch = "wasm32")]
    pub fn now() -> Instant {
        Instant(imp::now_millis())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.0.saturating_duration_since(earlier.0)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        Duration::from_secs_f64(((self.0 - earlier.0).max(0.0)) / 1000.0)
    }

    pub fn elapsed(&self) -> Duration {
        Instant::now().duration_since(*self)
    }
}

impl std::ops::Add<Duration> for Instant {
    type Output = Instant;
    #[cfg(not(target_arch = "wasm32"))]
    fn add(self, d: Duration) -> Instant {
        Instant(self.0 + d)
    }
    #[cfg(target_arch = "wasm32")]
    fn add(self, d: Duration) -> Instant {
        Instant(self.0 + d.as_secs_f64() * 1000.0)
    }
}

pub use imp::*;

/// Hosted requests never inherit owner or service environment configuration.
pub fn env_var(key: &str) -> Result<String, std::env::VarError> {
    if crate::vfs::is_scoped() { Err(std::env::VarError::NotPresent) } else { std::env::var(key) }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn a_command_past_the_timeout_is_killed_and_reads_exit_124() {
        let cwd = std::env::temp_dir();
        let started = std::time::Instant::now();
        let out = run_shell_within(
            "echo started; sleep 30; echo never",
            &cwd,
            Duration::from_millis(300),
        )
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(out.timed_out);
        assert!(!out.success);
        assert_eq!(out.code, 124);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "started");
        assert!(String::from_utf8_lossy(&out.stderr).contains("killed after 0s"));
    }

    // The timeout kills the whole process group: a background child of the script
    // that would outlive the shell is gone too, and holds no pipe open.
    #[cfg(unix)]
    #[test]
    fn a_timed_out_command_takes_its_children_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("survived");
        let script = format!(
            "(sleep 2; touch '{}') & echo $! > pid; sleep 30",
            marker.display()
        );
        let started = std::time::Instant::now();
        let out = run_shell_within(&script, dir.path(), Duration::from_millis(300)).unwrap();
        assert!(out.timed_out);
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        std::thread::sleep(Duration::from_secs(3));
        assert!(!marker.exists(), "a child of the script outlived the timeout");
    }

    #[test]
    fn a_finished_command_reports_its_code_and_both_streams() {
        let cwd = std::env::temp_dir();
        let out = run_shell_within("echo out; echo err 1>&2; exit 3", &cwd, Duration::from_secs(30))
            .unwrap();
        assert!(!out.timed_out);
        assert_eq!(out.code, 3);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "out");
        assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "err");
        assert_eq!(out.status(), "exit status: 3");
    }

    #[test]
    fn a_chatty_command_keeps_only_the_tail() {
        let cwd = std::env::temp_dir();
        let script = format!(
            "yes 0123456789abcdef | head -c {}; echo; echo last-line",
            SHELL_CAPTURE_BYTES * 2
        );
        let out = run_shell_within(&script, &cwd, Duration::from_secs(60)).unwrap();
        assert!(out.success);
        assert!(out.stdout.len() <= SHELL_CAPTURE_BYTES + 64, "{}", out.stdout.len());
        assert!(String::from_utf8_lossy(&out.stdout).trim_end().ends_with("last-line"));
    }
}
