//! Config validation CLI tool
//!
//! Validates a shepherdd configuration file and reports any errors.

use shepherd_api::EntryKind;
use shepherd_util::default_config_path;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    let config_path = match args.get(1) {
        Some(path) => PathBuf::from(path),
        None => {
            let default_path = default_config_path();
            eprintln!("Usage: validate-config [config-file]");
            eprintln!();
            eprintln!("Validates a shepherdd configuration file.");
            eprintln!();
            eprintln!("If no path is provided, uses: {}", default_path.display());
            eprintln!();
            eprintln!("Example:");
            eprintln!("  validate-config {}", default_path.display());
            eprintln!("  validate-config config.example.toml");
            return ExitCode::from(2);
        }
    };

    // Check file exists
    if !config_path.exists() {
        eprintln!("Error: Configuration file not found: {}", config_path.display());
        return ExitCode::from(1);
    }

    // Try to load and validate
    match shepherd_config::load_config(&config_path) {
        Ok(policy) => {
            println!("✓ Configuration is valid");
            println!();
            println!("Summary:");
            println!("  Config version: {}", shepherd_config::CURRENT_CONFIG_VERSION);
            println!("  Entries: {}", policy.entries.len());

            // Show entry summary
            if !policy.entries.is_empty() {
                println!();
                println!("Entries:");
                for entry in &policy.entries {
                    let kind_str = match &entry.kind {
                        EntryKind::Process { command, .. } => {
                            format!("process ({})", command)
                        }
                        EntryKind::Snap { snap_name, .. } => {
                            format!("snap ({})", snap_name)
                        }
                        EntryKind::Vm { driver, .. } => {
                            format!("vm ({})", driver)
                        }
                        EntryKind::Media { library_id, .. } => {
                            format!("media ({})", library_id)
                        }
                        EntryKind::Custom { type_name, .. } => {
                            format!("custom ({})", type_name)
                        }
                    };
                    println!("  - {} [{}]: {}", entry.id.as_str(), kind_str, entry.label);
                }
            }

            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("✗ Configuration validation failed");
            eprintln!();
            match &e {
                shepherd_config::ConfigError::ReadError(io_err) => {
                    eprintln!("Failed to read file: {}", io_err);
                }
                shepherd_config::ConfigError::ParseError(parse_err) => {
                    eprintln!("TOML parse error:");
                    eprintln!("  {}", parse_err);
                }
                shepherd_config::ConfigError::ValidationFailed { errors } => {
                    eprintln!("Validation errors ({}):", errors.len());
                    for err in errors {
                        eprintln!("  - {}", err);
                    }
                }
                shepherd_config::ConfigError::UnsupportedVersion(ver) => {
                    eprintln!(
                        "Unsupported config version: {} (expected {})",
                        ver,
                        shepherd_config::CURRENT_CONFIG_VERSION
                    );
                }
            }
            ExitCode::from(1)
        }
    }
}
