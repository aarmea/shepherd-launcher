//! Process management utilities

use nix::sys::signal::{self, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use tracing::{debug, info, warn};

use shepherd_host_api::{ExitStatus, HostError, HostResult};

/// Base path for shepherd's cgroups
const CGROUP_BASE: &str = "/sys/fs/cgroup/shepherd";

/// Managed child process with process group and optional cgroup
pub struct ManagedProcess {
    pub child: Child,
    pub pid: u32,
    pub pgid: u32,
    /// The cgroup path if cgroups are enabled
    pub cgroup_path: Option<PathBuf>,
}

/// Initialize the shepherd cgroup hierarchy (called once at startup)
pub fn init_cgroup_base() -> bool {
    let base = Path::new(CGROUP_BASE);
    
    // Check if cgroups v2 is available
    if !Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
        info!("cgroups v2 not available, falling back to process group signals");
        return false;
    }
    
    // Try to create our base cgroup
    if !base.exists() {
        if let Err(e) = std::fs::create_dir_all(base) {
            warn!(error = %e, "Failed to create shepherd cgroup base - running without cgroup support");
            return false;
        }
    }
    
    info!("cgroups v2 initialized at {}", CGROUP_BASE);
    true
}

/// Create a cgroup for a session
fn create_session_cgroup(session_id: &str) -> Option<PathBuf> {
    let cgroup_path = PathBuf::from(CGROUP_BASE).join(session_id);
    
    if let Err(e) = std::fs::create_dir_all(&cgroup_path) {
        warn!(error = %e, path = %cgroup_path.display(), "Failed to create session cgroup");
        return None;
    }
    
    debug!(path = %cgroup_path.display(), "Created session cgroup");
    Some(cgroup_path)
}

/// Move a process into a cgroup
fn move_to_cgroup(cgroup_path: &Path, pid: u32) -> bool {
    let procs_file = cgroup_path.join("cgroup.procs");
    
    if let Err(e) = std::fs::write(&procs_file, pid.to_string()) {
        warn!(error = %e, pid = pid, path = %procs_file.display(), "Failed to move process to cgroup");
        return false;
    }
    
    debug!(pid = pid, cgroup = %cgroup_path.display(), "Moved process to cgroup");
    true
}

/// Get all PIDs in a cgroup
fn get_cgroup_pids(cgroup_path: &Path) -> Vec<i32> {
    let procs_file = cgroup_path.join("cgroup.procs");
    
    match std::fs::read_to_string(&procs_file) {
        Ok(contents) => {
            contents
                .lines()
                .filter_map(|line| line.trim().parse::<i32>().ok())
                .collect()
        }
        Err(e) => {
            debug!(error = %e, path = %procs_file.display(), "Failed to read cgroup.procs");
            Vec::new()
        }
    }
}

/// Kill all processes in a cgroup
fn kill_cgroup(cgroup_path: &Path, signal: Signal) -> Vec<i32> {
    let pids = get_cgroup_pids(cgroup_path);
    
    for pid in &pids {
        let _ = signal::kill(Pid::from_raw(*pid), signal);
    }
    
    if !pids.is_empty() {
        debug!(pids = ?pids, signal = ?signal, cgroup = %cgroup_path.display(), "Sent signal to cgroup processes");
    }
    
    pids
}

/// Remove a session cgroup (must be empty)
fn cleanup_session_cgroup(cgroup_path: &Path) {
    // The cgroup must be empty before we can remove it
    // We'll try a few times in case processes are still exiting
    for _ in 0..5 {
        let pids = get_cgroup_pids(cgroup_path);
        if pids.is_empty() {
            if let Err(e) = std::fs::remove_dir(cgroup_path) {
                debug!(error = %e, path = %cgroup_path.display(), "Failed to remove session cgroup");
            } else {
                debug!(path = %cgroup_path.display(), "Removed session cgroup");
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    debug!(path = %cgroup_path.display(), "Cgroup still has processes, leaving cleanup for later");
}

impl ManagedProcess {
    /// Spawn a new process in its own process group and optionally in a cgroup
    pub fn spawn(
        argv: &[String],
        env: &HashMap<String, String>,
        cwd: Option<&PathBuf>,
        capture_output: bool,
    ) -> HostResult<Self> {
        Self::spawn_with_session_id(argv, env, cwd, capture_output, None)
    }
    
    /// Spawn a new process with an optional session ID for cgroup management
    pub fn spawn_with_session_id(
        argv: &[String],
        env: &HashMap<String, String>,
        cwd: Option<&PathBuf>,
        capture_output: bool,
        session_id: Option<&str>,
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

        // Special handling for WAYLAND_DISPLAY:
        // If SHEPHERD_WAYLAND_DISPLAY is set, use that instead of the inherited value.
        // This allows apps to be launched on a nested compositor while the daemon
        // runs on the parent compositor. When the daemon runs inside the nested
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

        // Configure output capture
        // For debugging, inherit stdout/stderr so we can see errors
        if capture_output {
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
        } else {
            // Inherit from parent so we can see child output for debugging
            cmd.stdout(Stdio::inherit());
            cmd.stderr(Stdio::inherit());
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
        
        // Try to create a cgroup for this session and move the process into it
        let cgroup_path = if let Some(sid) = session_id {
            if let Some(cg_path) = create_session_cgroup(sid) {
                if move_to_cgroup(&cg_path, pid) {
                    info!(pid = pid, cgroup = %cg_path.display(), "Process moved to session cgroup");
                    Some(cg_path)
                } else {
                    // Cleanup the empty cgroup we created
                    let _ = std::fs::remove_dir(&cg_path);
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        debug!(pid = pid, pgid = pgid, program = %program, has_cgroup = cgroup_path.is_some(), "Process spawned");

        Ok(Self { child, pid, pgid, cgroup_path })
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
                                if fields.len() >= 2 {
                                    if let Ok(ppid) = fields[1].parse::<i32>() {
                                        if ppid == parent_pid {
                                            descendants.push(pid);
                                            to_check.push(pid);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        descendants
    }

    /// Send SIGTERM to all processes in this session (via cgroup if available, or process group)
    pub fn terminate(&self) -> HostResult<()> {
        // If we have a cgroup, use it - this is the most reliable method
        if let Some(ref cgroup_path) = self.cgroup_path {
            let pids = kill_cgroup(cgroup_path, Signal::SIGTERM);
            info!(pids = ?pids, cgroup = %cgroup_path.display(), "Sent SIGTERM via cgroup");
            return Ok(());
        }
        
        // Fallback: try to kill the process group
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

    /// Send SIGKILL to all processes in this session (via cgroup if available, or process group)
    pub fn kill(&self) -> HostResult<()> {
        // If we have a cgroup, use it - this is the most reliable method
        if let Some(ref cgroup_path) = self.cgroup_path {
            let pids = kill_cgroup(cgroup_path, Signal::SIGKILL);
            info!(pids = ?pids, cgroup = %cgroup_path.display(), "Sent SIGKILL via cgroup");
            return Ok(());
        }
        
        // Fallback: try to kill the process group
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
    
    /// Clean up resources associated with this process (especially cgroups)
    pub fn cleanup(&self) {
        if let Some(ref cgroup_path) = self.cgroup_path {
            cleanup_session_cgroup(cgroup_path);
        }
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        // Try to clean up the cgroup when the process struct is dropped
        if let Some(ref cgroup_path) = self.cgroup_path {
            // Only try once, don't block in Drop
            let _ = std::fs::remove_dir(cgroup_path);
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
