//! Configuration parsing and validation for shepherdd
//!
//! Supports TOML configuration with:
//! - Versioned schema
//! - Entry definitions with availability policies
//! - Time windows, limits, and warnings
//! - Validation with clear error messages

mod policy;
mod schema;
mod validation;

pub use policy::*;
pub use schema::*;
pub use validation::*;

use std::path::Path;
use thiserror::Error;

/// Configuration errors
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    ReadError(#[from] std::io::Error),

    #[error("Failed to parse TOML: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Validation failed: {errors:?}")]
    ValidationFailed { errors: Vec<ValidationError> },

    #[error("Unsupported config version: {0}")]
    UnsupportedVersion(u32),
}

pub type ConfigResult<T> = Result<T, ConfigError>;

/// Load and validate configuration from a TOML file
pub fn load_config(path: impl AsRef<Path>) -> ConfigResult<Policy> {
    let content = std::fs::read_to_string(path)?;
    parse_config(&content)
}

/// Parse and validate configuration from a TOML string
pub fn parse_config(content: &str) -> ConfigResult<Policy> {
    let raw: RawConfig = toml::from_str(content)?;

    // Check version
    if raw.config_version != CURRENT_CONFIG_VERSION {
        return Err(ConfigError::UnsupportedVersion(raw.config_version));
    }

    // Validate
    let errors = validate_config(&raw);
    if !errors.is_empty() {
        return Err(ConfigError::ValidationFailed { errors });
    }

    // Convert to policy
    Ok(Policy::from_raw(raw))
}

/// Current supported config version
pub const CURRENT_CONFIG_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let config = r#"
            config_version = 1

            [[entries]]
            id = "test-game"
            label = "Test Game"
            kind = { type = "process", command = "/usr/bin/game" }
        "#;

        let policy = parse_config(config).unwrap();
        assert_eq!(policy.entries.len(), 1);
        assert_eq!(policy.entries[0].id.as_str(), "test-game");
    }

    #[test]
    fn reject_wrong_version() {
        let config = r#"
            config_version = 99

            [[entries]]
            id = "test"
            label = "Test"
            kind = { type = "process", command = "/bin/test" }
        "#;

        let result = parse_config(config);
        assert!(matches!(result, Err(ConfigError::UnsupportedVersion(99))));
    }
}
