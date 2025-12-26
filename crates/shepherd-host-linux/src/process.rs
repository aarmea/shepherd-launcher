//! Process management utilities

use nix::sys::signal::{self, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use tracing::{debug, warn};

use shepherd_host_api::{ExitStatus, HostError, HostResult};

/// Managed child process with process group
pub struct ManagedProcess {
    pub child: Child,
    pub pid: u32,
    pub pgid: u32,
}

impl ManagedProcess {
    /// Spawn a new process in its own process group
    pub fn spawn(
        argv: &[String],
        env: &HashMap<String, String>,
        cwd: Option<&PathBuf>,
        capture_output: bool,
    ) -> HostResult<Self> {
        if argv.is_empty() {
            return Err(HostError::SpawnFailed("Empty argv".into()));
        }

        let program = &argv[0];
        let args = &argv[1..];

        let mut cmd = Command::new(program);
        cmd.args(args);

        // Set environment
        cmd.env_clear();
        // Inherit some basic environment
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", path);
        }
        if let Ok(home) = std::env::var("HOME") {
            cmd.env("HOME", home);
        }
        if let Ok(display) = std::env::var("DISPLAY") {
            cmd.env("DISPLAY", display);
        }
        if let Ok(wayland) = std::env::var("WAYLAND_DISPLAY") {
            cmd.env("WAYLAND_DISPLAY", wayland);
        }
        if let Ok(xdg_runtime) = std::env::var("XDG_RUNTIME_DIR") {
            cmd.env("XDG_RUNTIME_DIR", xdg_runtime);
        }

        // Add custom environment
        for (k, v) in env {
            cmd.env(k, v);
        }

        // Set working directory
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        // Configure output capture
        if capture_output {
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
        } else {
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
        }

        cmd.stdin(Stdio::null());

        // Set up process group - this child becomes its own process group leader
        // SAFETY: This is safe in the pre-exec context
        unsafe {
            cmd.pre_exec(|| {
                // Create new session (which creates new process group)
                // This ensures the child is the leader of a new process group
                nix::unistd::setsid().map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                })?;
                Ok(())
            });
        }

        let child = cmd.spawn().map_err(|e| {
            HostError::SpawnFailed(format!("Failed to spawn {}: {}", program, e))
        })?;

        let pid = child.id();
        let pgid = pid; // After setsid, pid == pgid

        debug!(pid = pid, pgid = pgid, program = %program, "Process spawned");

        Ok(Self { child, pid, pgid })
    }

    /// Send SIGTERM to the process group
    pub fn terminate(&self) -> HostResult<()> {
        let pgid = Pid::from_raw(-(self.pgid as i32)); // Negative for process group

        match signal::kill(pgid, Signal::SIGTERM) {
            Ok(()) => {
                debug!(pgid = self.pgid, "Sent SIGTERM to process group");
                Ok(())
            }
            Err(nix::errno::Errno::ESRCH) => {
                // Process already gone
                Ok(())
            }
            Err(e) => Err(HostError::StopFailed(format!(
                "Failed to send SIGTERM: {}",
                e
            ))),
        }
    }

    /// Send SIGKILL to the process group
    pub fn kill(&self) -> HostResult<()> {
        let pgid = Pid::from_raw(-(self.pgid as i32));

        match signal::kill(pgid, Signal::SIGKILL) {
            Ok(()) => {
                debug!(pgid = self.pgid, "Sent SIGKILL to process group");
                Ok(())
            }
            Err(nix::errno::Errno::ESRCH) => {
                // Process already gone
                Ok(())
            }
            Err(e) => Err(HostError::StopFailed(format!(
                "Failed to send SIGKILL: {}",
                e
            ))),
        }
    }

    /// Check if the process has exited (non-blocking)
    pub fn try_wait(&mut self) -> HostResult<Option<ExitStatus>> {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                let exit_status = if let Some(code) = status.code() {
                    ExitStatus::with_code(code)
                } else {
                    // Killed by signal
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        if let Some(sig) = status.signal() {
                            ExitStatus::signaled(sig)
                        } else {
                            ExitStatus::with_code(-1)
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        ExitStatus::with_code(-1)
                    }
                };
                Ok(Some(exit_status))
            }
            Ok(None) => Ok(None), // Still running
            Err(e) => Err(HostError::Internal(format!("Wait failed: {}", e))),
        }
    }

    /// Wait for the process to exit (blocking)
    pub fn wait(&mut self) -> HostResult<ExitStatus> {
        match self.child.wait() {
            Ok(status) => {
                let exit_status = if let Some(code) = status.code() {
                    ExitStatus::with_code(code)
                } else {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        if let Some(sig) = status.signal() {
                            ExitStatus::signaled(sig)
                        } else {
                            ExitStatus::with_code(-1)
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        ExitStatus::with_code(-1)
                    }
                };
                Ok(exit_status)
            }
            Err(e) => Err(HostError::Internal(format!("Wait failed: {}", e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_simple_process() {
        let argv = vec!["true".to_string()];
        let env = HashMap::new();

        let mut proc = ManagedProcess::spawn(&argv, &env, None, false).unwrap();

        // Wait for it to complete
        let status = proc.wait().unwrap();
        assert!(status.is_success());
    }

    #[test]
    fn spawn_with_args() {
        let argv = vec!["echo".to_string(), "hello".to_string()];
        let env = HashMap::new();

        let mut proc = ManagedProcess::spawn(&argv, &env, None, false).unwrap();
        let status = proc.wait().unwrap();
        assert!(status.is_success());
    }

    #[test]
    fn terminate_sleeping_process() {
        let argv = vec!["sleep".to_string(), "60".to_string()];
        let env = HashMap::new();

        let proc = ManagedProcess::spawn(&argv, &env, None, false).unwrap();

        // Give it a moment to start
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Terminate it
        proc.terminate().unwrap();

        // Wait a bit and check
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Process should be gone or terminating
    }
}
