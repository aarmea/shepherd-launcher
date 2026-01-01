//! Default paths for shepherdd components
//!
//! Provides centralized path defaults that all crates can use.
//! Paths are user-writable by default (no root required):
//! - Socket: `$XDG_RUNTIME_DIR/shepherdd/shepherdd.sock` or `/tmp/shepherdd-$USER/shepherdd.sock`
//! - Data: `$XDG_DATA_HOME/shepherdd` or `~/.local/share/shepherdd`
//! - Logs: `$XDG_STATE_HOME/shepherdd` or `~/.local/state/shepherdd`

use std::path::PathBuf;

/// Environment variable for overriding the socket path
pub const SHEPHERD_SOCKET_ENV: &str = "SHEPHERD_SOCKET";

/// Environment variable for overriding the data directory
pub const SHEPHERD_DATA_DIR_ENV: &str = "SHEPHERD_DATA_DIR";

/// Socket filename within the socket directory
const SOCKET_FILENAME: &str = "shepherdd.sock";

/// Application subdirectory name
const APP_DIR: &str = "shepherdd";

/// Get the default socket path.
///
/// Order of precedence:
/// 1. `$SHEPHERD_SOCKET` environment variable (if set)
/// 2. `$XDG_RUNTIME_DIR/shepherdd/shepherdd.sock` (if XDG_RUNTIME_DIR is set)
/// 3. `/tmp/shepherdd-$USER/shepherdd.sock` (fallback)
pub fn default_socket_path() -> PathBuf {
    // Check environment override first
    if let Ok(path) = std::env::var(SHEPHERD_SOCKET_ENV) {
        return PathBuf::from(path);
    }

    socket_path_without_env()
}

/// Get the socket path without checking SHEPHERD_SOCKET env var.
/// Used for default values in configs where the env var is checked separately.
pub fn socket_path_without_env() -> PathBuf {
    // Try XDG_RUNTIME_DIR first (typically /run/user/<uid>)
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join(APP_DIR).join(SOCKET_FILENAME);
    }

    // Fallback to /tmp with username
    let username = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    PathBuf::from(format!("/tmp/{}-{}", APP_DIR, username)).join(SOCKET_FILENAME)
}

/// Get the default data directory.
///
/// Order of precedence:
/// 1. `$SHEPHERD_DATA_DIR` environment variable (if set)
/// 2. `$XDG_DATA_HOME/shepherdd` (if XDG_DATA_HOME is set)
/// 3. `~/.local/share/shepherdd` (fallback)
pub fn default_data_dir() -> PathBuf {
    // Check environment override first
    if let Ok(path) = std::env::var(SHEPHERD_DATA_DIR_ENV) {
        return PathBuf::from(path);
    }

    data_dir_without_env()
}

/// Get the data directory without checking SHEPHERD_DATA_DIR env var.
/// Used for default values in configs where the env var is checked separately.
pub fn data_dir_without_env() -> PathBuf {
    // Try XDG_DATA_HOME first
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join(APP_DIR);
    }

    // Fallback to ~/.local/share/shepherdd
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join(APP_DIR);
    }

    // Last resort
    PathBuf::from("/tmp").join(APP_DIR).join("data")
}

/// Get the default log directory.
///
/// Order of precedence:
/// 1. `$XDG_STATE_HOME/shepherdd` (if XDG_STATE_HOME is set)
/// 2. `~/.local/state/shepherdd` (fallback)
pub fn default_log_dir() -> PathBuf {
    // Try XDG_STATE_HOME first
    if let Ok(state_home) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(state_home).join(APP_DIR);
    }

    // Fallback to ~/.local/state/shepherdd
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join(APP_DIR);
    }

    // Last resort
    PathBuf::from("/tmp").join(APP_DIR).join("logs")
}

/// Get the parent directory of the socket (for creating it)
pub fn socket_dir() -> PathBuf {
    let socket_path = socket_path_without_env();
    socket_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| {
        // Should never happen with our paths, but just in case
        PathBuf::from("/tmp").join(APP_DIR)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_contains_shepherdd() {
        // The socket path should always contain "shepherdd" regardless of environment
        let path = socket_path_without_env();
        assert!(path.to_string_lossy().contains("shepherdd"));
        assert!(path.to_string_lossy().contains(".sock"));
    }

    #[test]
    fn data_dir_contains_shepherdd() {
        let path = data_dir_without_env();
        assert!(path.to_string_lossy().contains("shepherdd"));
    }

    #[test]
    fn log_dir_contains_shepherdd() {
        let path = default_log_dir();
        assert!(path.to_string_lossy().contains("shepherdd"));
    }

    #[test]
    fn socket_dir_is_parent_of_socket_path() {
        let socket = socket_path_without_env();
        let dir = socket_dir();
        assert_eq!(socket.parent().unwrap(), dir);
    }
}
