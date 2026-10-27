//! Configuration resolution by hand: flag over variable over default (decisions 42, 43, 45,
//! 47). The environment is injected as a closure so this module reads nothing itself; an
//! empty variable counts as unset, a variable that is not valid UTF-8 is an error, and every
//! error names its source (`--bind`, `NAGE_BIND`, `TZ`, ...) and quotes the value.

use std::ffi::OsString;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use jiff::tz::TimeZone;
use tracing_subscriber::EnvFilter;

use crate::cli::ServeArgs;

/// Reads one environment variable; `main` passes a closure over `std::env::var_os`.
pub(crate) type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// The bind address when neither `--bind` nor `NAGE_BIND` is given (SPEC 5.2).
const DEFAULT_BIND: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 8787);
/// The data directory when neither `--data-dir` nor `NAGE_DATA_DIR` is given (SPEC 5.2).
const DEFAULT_DATA_DIR: &str = "./data";
/// The time zone when neither `--tz`, `NAGE_TZ` nor `TZ` is given (decision 43).
const DEFAULT_TZ: &str = "UTC";
/// The log filter when neither `--log` nor `NAGE_LOG` is given (decision 47).
const DEFAULT_LOG: &str = "info";
/// The source named for a default value.
const DEFAULT: &str = "default";

/// Everything `nage serve` needs, resolved and validated.
pub(crate) struct ServeConfig {
    /// As given; `serve` makes it absolute against the working directory.
    pub(crate) data_dir: PathBuf,
    pub(crate) bind: SocketAddr,
    /// A zone known to the time-zone database, never the unknown zone.
    pub(crate) tz: TimeZone,
    /// `--tz`, `NAGE_TZ`, `TZ` or `default`.
    pub(crate) tz_source: &'static str,
    /// The filter directive as given, for the start-up line.
    pub(crate) log: String,
    /// The same directive, parsed.
    pub(crate) filter: EnvFilter,
    /// `NO_COLOR` is set to a non-empty value.
    pub(crate) no_color: bool,
}

/// A configuration error: exit code 2 (decision 45). Every variant carries its source.
#[derive(Debug)]
pub(crate) enum ConfigError {
    NotUnicode {
        source: &'static str,
        value: OsString,
    },
    InvalidAddress {
        source: &'static str,
        value: String,
    },
    PortZero {
        source: &'static str,
        value: String,
    },
    UnknownTimeZone {
        source: &'static str,
        value: String,
    },
    InvalidLogFilter {
        source: &'static str,
        value: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUnicode { source, value } => {
                write!(f, "{source}: not valid UTF-8 {value:?}")
            }
            Self::InvalidAddress { source, value } => {
                write!(f, "{source}: invalid address {value:?}")
            }
            Self::PortZero { source, value } => {
                write!(f, "{source}: port 0 is not a target {value:?}")
            }
            Self::UnknownTimeZone { source, value } => {
                write!(f, "{source}: unknown time zone {value:?}")
            }
            Self::InvalidLogFilter { source, value } => {
                write!(f, "{source}: invalid log filter {value:?}")
            }
        }
    }
}

/// Resolves the `serve` configuration (decisions 42, 43, 45, 47).
pub(crate) fn resolve_serve(args: ServeArgs, env: Env) -> Result<ServeConfig, ConfigError> {
    let bind = match pick(args.bind, "--bind", "NAGE_BIND", env)? {
        Some((value, source)) => parse_bind(&value, source)?,
        None => DEFAULT_BIND,
    };

    let data_dir = match args.data_dir {
        Some(path) => path,
        None => var("NAGE_DATA_DIR", env)?
            .map_or_else(|| PathBuf::from(DEFAULT_DATA_DIR), PathBuf::from),
    };

    let (tz_value, tz_source) = match args.tz {
        Some(value) => (value, "--tz"),
        None => match var("NAGE_TZ", env)? {
            Some(value) => (value, "NAGE_TZ"),
            None => match var("TZ", env)? {
                Some(value) => (value, "TZ"),
                None => (String::from(DEFAULT_TZ), DEFAULT),
            },
        },
    };
    let tz = time_zone(&tz_value, tz_source)?;

    let (log, log_source) = pick(args.log, "--log", "NAGE_LOG", env)?
        .unwrap_or_else(|| (String::from(DEFAULT_LOG), DEFAULT));
    let filter = EnvFilter::try_new(&log).map_err(|_| ConfigError::InvalidLogFilter {
        source: log_source,
        value: log.clone(),
    })?;

    let no_color = env("NO_COLOR").is_some_and(|value| !value.is_empty());

    Ok(ServeConfig {
        data_dir,
        bind,
        tz,
        tz_source,
        log,
        filter,
        no_color,
    })
}

/// Resolves the address `nage health` derives its target from (decision 44): an IP address
/// and port, never port 0.
pub(crate) fn resolve_health_bind(
    flag: Option<String>,
    env: Env,
) -> Result<SocketAddr, ConfigError> {
    let Some((value, source)) = pick(flag, "--bind", "NAGE_BIND", env)? else {
        return Ok(DEFAULT_BIND);
    };
    let bind = parse_bind(&value, source)?;
    if bind.port() == 0 {
        return Err(ConfigError::PortZero { source, value });
    }
    Ok(bind)
}

/// The flag when given, else the variable when set and non-empty, with its source.
fn pick(
    flag: Option<String>,
    flag_name: &'static str,
    variable: &'static str,
    env: Env,
) -> Result<Option<(String, &'static str)>, ConfigError> {
    if let Some(value) = flag {
        return Ok(Some((value, flag_name)));
    }
    Ok(var(variable, env)?.map(|value| (value, variable)))
}

/// A variable's value; empty counts as unset, non-UTF-8 is an error.
fn var(name: &'static str, env: Env) -> Result<Option<String>, ConfigError> {
    match env(name) {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) => value
            .into_string()
            .map(Some)
            .map_err(|value| ConfigError::NotUnicode {
                source: name,
                value,
            }),
    }
}

/// An IP address and port, IPv4 or bracketed IPv6; no host names (decision 45).
fn parse_bind(value: &str, source: &'static str) -> Result<SocketAddr, ConfigError> {
    value.parse().map_err(|_| ConfigError::InvalidAddress {
        source,
        value: value.to_owned(),
    })
}

/// A name known to the time-zone database; `Etc/Unknown`, POSIX rule strings and paths are
/// refused (decision 43).
fn time_zone(value: &str, source: &'static str) -> Result<TimeZone, ConfigError> {
    match TimeZone::get(value) {
        Ok(tz) if !tz.is_unknown() && tz.iana_name().is_some() => Ok(tz),
        _ => Err(ConfigError::UnknownTimeZone {
            source,
            value: value.to_owned(),
        }),
    }
}
