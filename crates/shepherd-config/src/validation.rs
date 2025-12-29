//! Configuration validation

use crate::schema::{RawConfig, RawDays, RawEntry, RawEntryKind, RawTimeWindow};
use std::collections::HashSet;
use thiserror::Error;

/// Validation error
#[derive(Debug, Clone, Error)]
pub enum ValidationError {
    #[error("Entry '{entry_id}': {message}")]
    EntryError { entry_id: String, message: String },

    #[error("Duplicate entry ID: {0}")]
    DuplicateEntryId(String),

    #[error("Invalid time format '{value}': {message}")]
    InvalidTimeFormat { value: String, message: String },

    #[error("Invalid day specification: {0}")]
    InvalidDaySpec(String),

    #[error("Warning threshold {seconds}s >= max_run {max_run}s for entry '{entry_id}'")]
    WarningExceedsMaxRun {
        entry_id: String,
        seconds: u64,
        max_run: u64,
    },

    #[error("Global config error: {0}")]
    GlobalError(String),
}

/// Validate a raw configuration
pub fn validate_config(config: &RawConfig) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Check for duplicate entry IDs
    let mut seen_ids = HashSet::new();
    for entry in &config.entries {
        if !seen_ids.insert(&entry.id) {
            errors.push(ValidationError::DuplicateEntryId(entry.id.clone()));
        }
    }

    // Validate each entry
    for entry in &config.entries {
        errors.extend(validate_entry(entry, config));
    }

    errors
}

fn validate_entry(entry: &RawEntry, config: &RawConfig) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Validate kind
    match &entry.kind {
        RawEntryKind::Process { command, .. } => {
            if command.is_empty() {
                errors.push(ValidationError::EntryError {
                    entry_id: entry.id.clone(),
                    message: "command cannot be empty".into(),
                });
            }
        }
        RawEntryKind::Snap { snap_name, .. } => {
            if snap_name.is_empty() {
                errors.push(ValidationError::EntryError {
                    entry_id: entry.id.clone(),
                    message: "snap_name cannot be empty".into(),
                });
            }
        }
        RawEntryKind::Vm { driver, .. } => {
            if driver.is_empty() {
                errors.push(ValidationError::EntryError {
                    entry_id: entry.id.clone(),
                    message: "VM driver cannot be empty".into(),
                });
            }
        }
        RawEntryKind::Media { library_id, .. } => {
            if library_id.is_empty() {
                errors.push(ValidationError::EntryError {
                    entry_id: entry.id.clone(),
                    message: "library_id cannot be empty".into(),
                });
            }
        }
        RawEntryKind::Custom { type_name, .. } => {
            if type_name.is_empty() {
                errors.push(ValidationError::EntryError {
                    entry_id: entry.id.clone(),
                    message: "type_name cannot be empty".into(),
                });
            }
        }
    }

    // Validate availability windows
    if let Some(avail) = &entry.availability {
        for window in &avail.windows {
            errors.extend(validate_time_window(window, &entry.id));
        }
    }

    // Validate warning thresholds vs max_run
    // Skip validation if max_run is 0 (unlimited) since there's no expiry to warn about
    let max_run = entry
        .limits
        .as_ref()
        .and_then(|l| l.max_run_seconds)
        .or(config.service.default_max_run_seconds);

    // Only validate warnings if max_run is Some and not 0 (unlimited)
    if let (Some(warnings), Some(max_run)) = (&entry.warnings, max_run)
        && max_run > 0 {
            for warning in warnings {
                if warning.seconds_before >= max_run {
                    errors.push(ValidationError::WarningExceedsMaxRun {
                        entry_id: entry.id.clone(),
                        seconds: warning.seconds_before,
                        max_run,
                    });
                }
            }
        // Note: warnings are ignored for unlimited entries (max_run = 0)
    }

    errors
}

fn validate_time_window(window: &RawTimeWindow, entry_id: &str) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Validate days
    if let Err(e) = parse_days(&window.days) {
        errors.push(ValidationError::EntryError {
            entry_id: entry_id.to_string(),
            message: e,
        });
    }

    // Validate start time
    if let Err(e) = parse_time(&window.start) {
        errors.push(ValidationError::InvalidTimeFormat {
            value: window.start.clone(),
            message: e,
        });
    }

    // Validate end time
    if let Err(e) = parse_time(&window.end) {
        errors.push(ValidationError::InvalidTimeFormat {
            value: window.end.clone(),
            message: e,
        });
    }

    errors
}

/// Parse HH:MM time format
pub fn parse_time(s: &str) -> Result<(u8, u8), String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err("Expected HH:MM format".into());
    }

    let hour: u8 = parts[0]
        .parse()
        .map_err(|_| "Invalid hour".to_string())?;
    let minute: u8 = parts[1]
        .parse()
        .map_err(|_| "Invalid minute".to_string())?;

    if hour >= 24 {
        return Err("Hour must be 0-23".into());
    }
    if minute >= 60 {
        return Err("Minute must be 0-59".into());
    }

    Ok((hour, minute))
}

/// Parse days specification
pub fn parse_days(days: &RawDays) -> Result<u8, String> {
    match days {
        RawDays::Preset(preset) => match preset.to_lowercase().as_str() {
            "all" | "every" | "daily" => Ok(0x7F),
            "weekdays" => Ok(0x1F), // Mon-Fri
            "weekends" => Ok(0x60), // Sat-Sun
            other => Err(format!("Unknown day preset: {}", other)),
        },
        RawDays::List(list) => {
            let mut mask = 0u8;
            for day in list {
                let bit = match day.to_lowercase().as_str() {
                    "mon" | "monday" => 1 << 0,
                    "tue" | "tuesday" => 1 << 1,
                    "wed" | "wednesday" => 1 << 2,
                    "thu" | "thursday" => 1 << 3,
                    "fri" | "friday" => 1 << 4,
                    "sat" | "saturday" => 1 << 5,
                    "sun" | "sunday" => 1 << 6,
                    other => return Err(format!("Unknown day: {}", other)),
                };
                mask |= bit;
            }
            Ok(mask)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time() {
        assert_eq!(parse_time("14:30").unwrap(), (14, 30));
        assert_eq!(parse_time("00:00").unwrap(), (0, 0));
        assert_eq!(parse_time("23:59").unwrap(), (23, 59));

        assert!(parse_time("24:00").is_err());
        assert!(parse_time("12:60").is_err());
        assert!(parse_time("invalid").is_err());
    }

    #[test]
    fn test_parse_days() {
        assert_eq!(parse_days(&RawDays::Preset("weekdays".into())).unwrap(), 0x1F);
        assert_eq!(parse_days(&RawDays::Preset("weekends".into())).unwrap(), 0x60);
        assert_eq!(parse_days(&RawDays::Preset("all".into())).unwrap(), 0x7F);

        assert_eq!(
            parse_days(&RawDays::List(vec!["mon".into(), "wed".into(), "fri".into()])).unwrap(),
            0b10101
        );
    }

    #[test]
    fn test_duplicate_id_detection() {
        let config = RawConfig {
            config_version: 1,
            service: Default::default(),
            entries: vec![
                RawEntry {
                    id: "game".into(),
                    label: "Game 1".into(),
                    icon: None,
                    kind: RawEntryKind::Process {
                        command: "game1".into(),
                        args: vec![],
                        env: Default::default(),
                        cwd: None,
                    },
                    availability: None,
                    limits: None,
                    warnings: None,
                    volume: None,
                    disabled: false,
                    disabled_reason: None,
                },
                RawEntry {
                    id: "game".into(),
                    label: "Game 2".into(),
                    icon: None,
                    kind: RawEntryKind::Process {
                        command: "game2".into(),
                        args: vec![],
                        env: Default::default(),
                        cwd: None,
                    },
                    availability: None,
                    limits: None,
                    warnings: None,
                    volume: None,
                    disabled: false,
                    disabled_reason: None,
                },
            ],
        };

        let errors = validate_config(&config);
        assert!(errors.iter().any(|e| matches!(e, ValidationError::DuplicateEntryId(_))));
    }
}
