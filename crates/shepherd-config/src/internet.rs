//! Internet connectivity configuration and parsing.

use std::time::Duration;

/// Default interval between connectivity checks.
pub const DEFAULT_INTERNET_CHECK_INTERVAL: Duration = Duration::from_secs(10);
/// Default timeout for a single connectivity check.
pub const DEFAULT_INTERNET_CHECK_TIMEOUT: Duration = Duration::from_millis(1500);

/// Supported connectivity check schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InternetCheckScheme {
    Tcp,
    Http,
    Https,
}

impl InternetCheckScheme {
    fn from_str(value: &str) -> Result<Self, String> {
        match value.to_lowercase().as_str() {
            "tcp" => Ok(Self::Tcp),
            "http" => Ok(Self::Http),
            "https" => Ok(Self::Https),
            other => Err(format!("unsupported scheme '{}'", other)),
        }
    }

    fn default_port(self) -> u16 {
        match self {
            Self::Tcp => 0,
            Self::Http => 80,
            Self::Https => 443,
        }
    }
}

/// Connectivity check target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InternetCheckTarget {
    pub scheme: InternetCheckScheme,
    pub host: String,
    pub port: u16,
    pub original: String,
}

impl InternetCheckTarget {
    /// Parse a connectivity check string (e.g., "https://example.com" or "tcp://1.1.1.1:53").
    pub fn parse(value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("check target cannot be empty".into());
        }

        let (scheme_raw, rest) = trimmed
            .split_once("://")
            .ok_or_else(|| "missing scheme (expected scheme://)".to_string())?;

        let scheme = InternetCheckScheme::from_str(scheme_raw)?;
        let rest = rest.trim();
        if rest.is_empty() {
            return Err("missing host".into());
        }

        let host_port = rest.split('/').next().unwrap_or(rest);
        let (host, port_opt) = parse_host_port(host_port)?;

        let port = match scheme {
            InternetCheckScheme::Tcp => port_opt
                .ok_or_else(|| "tcp check requires explicit port".to_string())?,
            _ => port_opt.unwrap_or_else(|| scheme.default_port()),
        };

        if port == 0 {
            return Err("invalid port".into());
        }

        Ok(Self {
            scheme,
            host,
            port,
            original: trimmed.to_string(),
        })
    }
}

fn parse_host_port(value: &str) -> Result<(String, Option<u16>), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("missing host".into());
    }

    if trimmed.starts_with('[') {
        let end = trimmed
            .find(']')
            .ok_or_else(|| "invalid IPv6 host".to_string())?;
        let host = trimmed[1..end].trim();
        if host.is_empty() {
            return Err("missing host".into());
        }
        let port = if let Some(port_str) = trimmed[end + 1..].strip_prefix(':') {
            Some(parse_port(port_str)?)
        } else {
            None
        };
        return Ok((host.to_string(), port));
    }

    let mut parts = trimmed.splitn(2, ':');
    let host = parts.next().unwrap_or("").trim();
    if host.is_empty() {
        return Err("missing host".into());
    }
    let port = parts.next().map(parse_port).transpose()?;
    Ok((host.to_string(), port))
}

fn parse_port(value: &str) -> Result<u16, String> {
    let port: u16 = value
        .trim()
        .parse()
        .map_err(|_| "invalid port".to_string())?;
    if port == 0 {
        return Err("invalid port".into());
    }
    Ok(port)
}

/// Service-level internet connectivity configuration.
#[derive(Debug, Clone)]
pub struct InternetConfig {
    pub check: Option<InternetCheckTarget>,
    pub interval: Duration,
    pub timeout: Duration,
}

impl InternetConfig {
    pub fn new(check: Option<InternetCheckTarget>, interval: Duration, timeout: Duration) -> Self {
        Self {
            check,
            interval,
            timeout,
        }
    }
}

/// Entry-level internet requirement.
#[derive(Debug, Clone, Default)]
pub struct EntryInternetPolicy {
    pub required: bool,
    pub check: Option<InternetCheckTarget>,
}

