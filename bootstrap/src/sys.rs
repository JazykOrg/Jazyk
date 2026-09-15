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

    pub fn run_shell(script: &str, cwd: &Path) -> std::io::Result<ShellOutput> {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(cwd)
            .output()?;
        Ok(ShellOutput {
            success: out.status.success(),
            code: out.status.code().unwrap_or(-1),
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }

    pub fn process_id() -> u32 {
        std::process::id()
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
