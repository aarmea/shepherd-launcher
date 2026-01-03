//! Process management utilities

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use tracing::{debug, info, warn};

use shepherd_host_api::{ExitStatus, HostError, HostResult};

/// Managed child process with process group tracking
pub struct ManagedProcess {
    pub child: Child,
    pub pid: u32,
    pub pgid: u32,
    /// The command name (for fallback killing via pkill)
    pub command_name: String,
    /// The snap name if this is a snap app (for cgroup-based killing)
    pub snap_name: Option<String>,
}

/// Initialize process management (called once at startup)
pub fn init() {
    info!("Process management initialized");
}

/// Kill all processes in a snap's cgroup using systemd
/// Snaps create scopes at: snap.<snap-name>.<snap-name>-<uuid>.scope
/// Direct signals don't work due to AppArmor confinement, but systemctl --user does
/// NOTE: We always use SIGKILL for snap apps because apps like Minecraft Launcher
/// have self-restart behavior and will spawn new instances when receiving SIGTERM
pub fn kill_snap_cgroup(snap_name: &str, _signal: Signal) -> bool {
    let uid = nix::unistd::getuid().as_raw();
    let base_path = format!(
        "/sys/fs/cgroup/user.slice/user-{}.slice/user@{}.service/app.slice",
        uid, uid
    );
    
    // Find all scope directories matching this snap
    let pattern = format!("snap.{}.{}-", snap_name, snap_name);
    
    let base = std::path::Path::new(&base_path);
    if !base.exists() {
        debug!(path = %base_path, "Snap cgroup base path doesn't exist");
        return false;
    }
    
    let mut stopped_any = false;
    
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            
            if name_str.starts_with(&pattern) && name_str.ends_with(".scope") {
                let scope_name = name_str.to_string();
                
                // Always use SIGKILL for snap apps to prevent self-restart behavior
                // Using systemctl kill --signal=KILL sends SIGKILL to all processes in scope
                let result = Command::new("systemctl")
                    .args(["--user", "kill", "--signal=KILL", &scope_name])
                    .output();
                
                match result {
                    Ok(output) => {
                        if output.status.success() {
                            info!(scope = %scope_name, "Killed snap scope via systemctl SIGKILL");
                            stopped_any = true;
                        } else {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            warn!(scope = %scope_name, stderr = %stderr, "systemctl kill command failed");
                        }
                    }
                    Err(e) => {
                        warn!(scope = %scope_name, error = %e, "Failed to run systemctl");
                    }
                }
            }
        }
    }
    
    if stopped_any {
        info!(snap = snap_name, "Killed snap scope(s) via systemctl SIGKILL");
    } else {
        debug!(snap = snap_name, "No snap scope found to kill");
    }
    
    stopped_any
}

/// Kill processes by command name using pkill
pub fn kill_by_command(command_name: &str, signal: Signal) -> bool {
    let signal_name = match signal {
        Signal::SIGTERM => "TERM",
        Signal::SIGKILL => "KILL",
        _ => "TERM",
    };
    
    // Use pkill to find and kill processes by command name
    let result = Command::new("pkill")
        .args([&format!("-{}", signal_name), "-f", command_name])
        .output();
    
    match result {
        Ok(output) => {
            // pkill returns 0 if processes were found and signaled
            if output.status.success() {
                info!(command = command_name, signal = signal_name, "Killed processes by command name");
                true
            } else {
                // No processes found is not an error
                debug!(command = command_name, "No processes found matching command name");
                false
            }
        }
        Err(e) => {
            warn!(command = command_name, error = %e, "Failed to run pkill");
            false
        }
    }
}

impl ManagedProcess {
    /// Spawn a new process in its own process group
    /// 
    /// If `snap_name` is provided, the process is treated as a snap app and will use
    /// systemd scope-based killing instead of signal-based killing.
    /// 
    /// If `log_path` is provided, stdout and stderr will be redirected to that file.
    /// For snap apps, we use `script` to capture output from all child processes
    /// via a pseudo-terminal, since snap child processes don't inherit file descriptors.
    pub fn spawn(
        argv: &[String],
        env: &HashMap<String, String>,
        cwd: Option<&std::path::PathBuf>,
        log_path: Option<PathBuf>,
        snap_name: Option<String>,
    ) -> HostResult<Self> {
        if argv.is_empty() {
            return Err(HostError::SpawnFailed("Empty argv".into()));
        }

        // For snap apps with log capture, wrap with `script` to capture all child output
        // via a pseudo-terminal. Snap child processes don't inherit file descriptors,
        // but they do write to the controlling terminal.
        let (actual_argv, actual_log_path) = match (&snap_name, &log_path) {
            (Some(_), Some(log_file)) => {
                // Create parent directory if it doesn't exist
                if let Some(parent) = log_file.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    warn!(path = %parent.display(), error = %e, "Failed to create log directory");
                }
                
                // Build command: script -q -c "original command" logfile
                // -q: quiet mode (no start/done messages)
                // -c: command to run
                let original_cmd = argv.iter()
                    .map(|arg| shell_escape::escape(std::borrow::Cow::Borrowed(arg)))
                    .collect::<Vec<_>>()
                    .join(" ");
                
                let script_argv = vec![
                    "script".to_string(),
                    "-q".to_string(),
                    "-c".to_string(),
                    original_cmd,
                    log_file.to_string_lossy().to_string(),
                ];
                
                info!(log_path = %log_file.display(), "Using script to capture snap output via pty");
                (script_argv, None) // script handles the log file itself
            }
            _ => (argv.to_vec(), log_path),
        };

        let program = &actual_argv[0];
        let args = &actual_argv[1..];

        let mut cmd = Command::new(program);
        cmd.args(args);

        // Set environment
        cmd.env_clear();
        
        // Inherit essential environment variables
        // These are needed for most Linux applications to work correctly
        let inherit_vars = [
            // Core paths
            "PATH",
            "HOME",
            "USER",
            "SHELL",
            // Display/graphics - both X11 and Wayland
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
            "XDG_SESSION_TYPE",
            "XDG_SESSION_DESKTOP",
            "XDG_CURRENT_DESKTOP",
            // X11 authorization (needed for XWayland apps)
            "XAUTHORITY",
            // XDG directories (needed for app data/config)
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "XDG_DATA_DIRS",
            "XDG_CONFIG_DIRS",
            // Snap support (critical for Snap apps like Minecraft)
            "SNAP",
            "SNAP_USER_DATA",
            "SNAP_USER_COMMON",
            "SNAP_REAL_HOME",
            "SNAP_NAME",
            "SNAP_INSTANCE_NAME",
            "SNAP_ARCH",
            "SNAP_VERSION",
            "SNAP_REVISION",
            "SNAP_COMMON",
            "SNAP_DATA",
            "SNAP_LIBRARY_PATH",
            // Locale
            "LANG",
            "LANGUAGE",
            "LC_ALL",
            // D-Bus (needed for many GUI apps)
            "DBUS_SESSION_BUS_ADDRESS",
            // Graphics/GPU
            "LIBGL_ALWAYS_SOFTWARE",
            "__GLX_VENDOR_LIBRARY_NAME",
            "VK_ICD_FILENAMES",
            "MESA_LOADER_DRIVER_OVERRIDE",
            // Audio
            "PULSE_SERVER",
            "PULSE_COOKIE",
            // GTK/GLib settings (needed for proper theming and SSL)
            "GTK_MODULES",
            "GIO_EXTRA_MODULES",
            "GSETTINGS_SCHEMA_DIR",
            "GSETTINGS_BACKEND",
            // SSL/TLS certificate locations
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
            "CURL_CA_BUNDLE",
            "REQUESTS_CA_BUNDLE",
            // Desktop session info (needed for portal integration)
            "DESKTOP_SESSION",
            "GNOME_DESKTOP_SESSION_ID",
        ];
        
        for var in inherit_vars {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        // Java AWT/Swing applications (like Minecraft) need this to work properly
        // on non-reparenting window managers like Sway. Without this, Java apps may
        // have focus issues or render incorrectly.
        cmd.env("_JAVA_AWT_WM_NONREPARENTING", "1");

        // Special handling for WAYLAND_DISPLAY:
        // If SHEPHERD_WAYLAND_DISPLAY is set, use that instead of the inherited value.
        // This allows apps to be launched on a nested compositor while the service
        // runs on the parent compositor. When the service runs inside the nested
        // compositor, this is not needed as WAYLAND_DISPLAY is already correct.
        if let Ok(shepherd_display) = std::env::var("SHEPHERD_WAYLAND_DISPLAY") {
            debug!(display = %shepherd_display, "Using SHEPHERD_WAYLAND_DISPLAY override for child process");
            cmd.env("WAYLAND_DISPLAY", shepherd_display);
        }

        // Add custom environment (these can override inherited vars)
        for (k, v) in env {
            cmd.env(k, v);
        }

        // Set working directory
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        // Configure output handling
        // If actual_log_path is provided, redirect stdout/stderr to the log file
        // (For snap apps, we already wrapped with `script` which handles logging)
        // Otherwise, inherit from parent so we can see child output for debugging
        if let Some(ref path) = actual_log_path {
            // Create parent directory if it doesn't exist
            if let Some(parent) = path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                warn!(path = %parent.display(), error = %e, "Failed to create log directory");
            }
            
            // Open log file for appending (create if doesn't exist)
            match File::create(path) {
                Ok(file) => {
                    // Clone file handle for stderr (both point to same file)
                    let stderr_file = match file.try_clone() {
                        Ok(f) => f,
                        Err(e) => {
                            warn!(path = %path.display(), error = %e, "Failed to clone log file handle");
                            cmd.stdout(Stdio::inherit());
                            cmd.stderr(Stdio::inherit());
                            cmd.stdin(Stdio::null());
                            // Skip to spawn
                            return Self::spawn_with_cmd(cmd, program, snap_name);
                        }
                    };
                    cmd.stdout(Stdio::from(file));
                    cmd.stderr(Stdio::from(stderr_file));
                    info!(path = %path.display(), "Redirecting child output to log file");
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Failed to open log file, inheriting output");
                    cmd.stdout(Stdio::inherit());
                    cmd.stderr(Stdio::inherit());
                }
            }
        } else {
            // Inherit from parent so we can see child output for debugging
            cmd.stdout(Stdio::inherit());
            cmd.stderr(Stdio::inherit());
        }

        cmd.stdin(Stdio::null());

        Self::spawn_with_cmd(cmd, program, snap_name)
    }

    /// Complete the spawn process with the configured command
    fn spawn_with_cmd(
        mut cmd: Command,
        program: &str,
        snap_name: Option<String>,
    ) -> HostResult<Self> {
        // Store the command name for later use in killing
        let command_name = program.to_string();

        // Set up process group - this child becomes its own process group leader
        // SAFETY: This is safe in the pre-exec context
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid().map_err(|e| {
                    std::io::Error::other(e.to_string())
                })?;
                Ok(())
            });
        }

        let child = cmd.spawn().map_err(|e| {
            HostError::SpawnFailed(format!("Failed to spawn {}: {}", program, e))
        })?;

        let pid = child.id();
        let pgid = pid; // After setsid, pid == pgid
        
        info!(pid = pid, pgid = pgid, program = %program, snap = ?snap_name, "Process spawned");

        Ok(Self { child, pid, pgid, command_name, snap_name })
    }

    /// Get all descendant PIDs of this process using /proc
    fn get_descendant_pids(&self) -> Vec<i32> {
        let mut descendants = Vec::new();
        let mut to_check = vec![self.pid as i32];
        
        while let Some(parent_pid) = to_check.pop() {
            // Read /proc to find children of this PID
            if let Ok(entries) = std::fs::read_dir("/proc") {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    
                    // Skip non-numeric entries (not PIDs)
                    if let Ok(pid) = name_str.parse::<i32>() {
                        // Read the stat file to get parent PID
                        let stat_path = format!("/proc/{}/stat", pid);
                        if let Ok(stat) = std::fs::read_to_string(&stat_path) {
                            // Format: pid (comm) state ppid ...
                            // Find the closing paren to handle comm with spaces/parens
                            if let Some(paren_end) = stat.rfind(')') {
                                let after_comm = &stat[paren_end + 2..];
                                let fields: Vec<&str> = after_comm.split_whitespace().collect();
                                if fields.len() >= 2
                                    && let Ok(ppid) = fields[1].parse::<i32>()
                                    && ppid == parent_pid {
                                        descendants.push(pid);
                                        to_check.push(pid);
                                    }
                            }
                        }
                    }
                }
            }
        }
        
        descendants
    }

    /// Send SIGTERM to all processes in this session
    pub fn terminate(&self) -> HostResult<()> {
        // For snap apps, we rely on cgroup-based killing in the adapter, not pkill
        // Using pkill with broad patterns like "snap" would kill unrelated processes
        if self.snap_name.is_none() {
            kill_by_command(&self.command_name, Signal::SIGTERM);
        }
        
        // Also try to kill the process group
        let pgid = Pid::from_raw(-(self.pgid as i32)); // Negative for process group

        match signal::kill(pgid, Signal::SIGTERM) {
            Ok(()) => {
                debug!(pgid = self.pgid, "Sent SIGTERM to process group");
            }
            Err(nix::errno::Errno::ESRCH) => {
                // Process group already gone
            }
            Err(e) => {
                debug!(pgid = self.pgid, error = %e, "Failed to send SIGTERM to process group");
            }
        }
        
        // Also kill all descendants (they may have escaped the process group)
        let descendants = self.get_descendant_pids();
        for pid in &descendants {
            let _ = signal::kill(Pid::from_raw(*pid), Signal::SIGTERM);
        }
        if !descendants.is_empty() {
            debug!(descendants = ?descendants, "Sent SIGTERM to descendant processes");
        }
        
        Ok(())
    }

    /// Send SIGKILL to all processes in this session
    pub fn kill(&self) -> HostResult<()> {
        // For snap apps, we rely on cgroup-based killing in the adapter, not pkill
        // Using pkill with broad patterns like "snap" would kill unrelated processes
        if self.snap_name.is_none() {
            kill_by_command(&self.command_name, Signal::SIGKILL);
        }
        
        // Also try to kill the process group
        let pgid = Pid::from_raw(-(self.pgid as i32));

        match signal::kill(pgid, Signal::SIGKILL) {
            Ok(()) => {
                debug!(pgid = self.pgid, "Sent SIGKILL to process group");
            }
            Err(nix::errno::Errno::ESRCH) => {
                // Process group already gone
            }
            Err(e) => {
                debug!(pgid = self.pgid, error = %e, "Failed to send SIGKILL to process group");
            }
        }
        
        // Also kill all descendants (they may have escaped the process group)
        let descendants = self.get_descendant_pids();
        for pid in &descendants {
            let _ = signal::kill(Pid::from_raw(*pid), Signal::SIGKILL);
        }
        if !descendants.is_empty() {
            debug!(descendants = ?descendants, "Sent SIGKILL to descendant processes");
        }
        
        Ok(())
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
    
    /// Clean up resources associated with this process
    pub fn cleanup(&self) {
        // Nothing to clean up for systemd scopes - systemd handles it
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        // Nothing special to do for systemd scopes - systemd cleans up automatically
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_simple_process() {
        let argv = vec!["true".to_string()];
        let env = HashMap::new();

        let mut proc = ManagedProcess::spawn(&argv, &env, None, None, None).unwrap();

        // Wait for it to complete
        let status = proc.wait().unwrap();
        assert!(status.is_success());
    }

    #[test]
    fn spawn_with_args() {
        let argv = vec!["echo".to_string(), "hello".to_string()];
        let env = HashMap::new();

        let mut proc = ManagedProcess::spawn(&argv, &env, None, None, None).unwrap();
        let status = proc.wait().unwrap();
        assert!(status.is_success());
    }

    #[test]
    fn terminate_sleeping_process() {
        let argv = vec!["sleep".to_string(), "60".to_string()];
        let env = HashMap::new();

        let proc = ManagedProcess::spawn(&argv, &env, None, None, None).unwrap();

        // Give it a moment to start
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Terminate it
        proc.terminate().unwrap();

        // Wait a bit and check
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Process should be gone or terminating
    }
}
