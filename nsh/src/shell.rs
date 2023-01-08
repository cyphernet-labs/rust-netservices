// LNP/BP Core Library implementing LNPBP specifications & standards
// Written in 2020 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the MIT License
// along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

//! Traits and structures simplifying creation of executable files, either for
//! daemons or command-line tools

use std::path::PathBuf;
use std::str::FromStr;
use std::{env, fs};

use log::LevelFilter;

/// Represents desired logging verbodity level
#[derive(Copy, Clone, PartialEq, Eq, Debug, Display)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate", rename_all = "lowercase")
)]
pub enum LogLevel {
    /// Report only errors to `stderr` and normat program output to stdin
    /// (if it is not directed to a file). Corresponds to zero verbosity
    /// flags.
    #[display("error")]
    Error = 0,

    /// Report warning messages and errors, plus standard program output.
    /// Corresponds to a single `-v` verbosity flag.
    #[display("warn")]
    Warn,

    /// Report genetic information messages, warnings and errors.
    /// Corresponds to a double `-vv` verbosity flag.
    #[display("info")]
    Info,

    /// Report debugging information and all non-trace messages, including
    /// general information, warnings and errors.
    /// Corresponds to triple `-vvv` verbosity flag.
    #[display("debug")]
    Debug,

    /// Print all possible messages including tracing information.
    /// Corresponds to quadruple `-vvvv` verbosity flag.
    #[display("trace")]
    Trace,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Display, Error)]
#[display(doc_comments)]
/// Unrecognized value `{_0}` for log level type; allowed values are
/// `error`, `warn`, `info`, `debug`, `trace`
pub struct LogLevelParseError(String);

impl FromStr for LogLevel {
    type Err = LogLevelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_ref() {
            "error" | "errors" => LogLevel::Error,
            "warn" | "warning" | "warnings" => LogLevel::Warn,
            "info" => LogLevel::Info,
            "debug" => LogLevel::Debug,
            "trace" | "tracing" => LogLevel::Trace,
            other => Err(LogLevelParseError(other.to_owned()))?,
        })
    }
}

impl From<u8> for LogLevel {
    fn from(val: u8) -> Self {
        Self::from_verbosity_flag_count(val)
    }
}

impl From<LogLevel> for u8 {
    fn from(log_level: LogLevel) -> Self {
        log_level.verbosity_flag_count()
    }
}

impl LogLevel {
    /// Indicates number of required verbosity flags
    pub fn verbosity_flag_count(&self) -> u8 {
        match self {
            LogLevel::Error => 0,
            LogLevel::Warn => 1,
            LogLevel::Info => 2,
            LogLevel::Debug => 3,
            LogLevel::Trace => 4,
        }
    }

    /// Constructs enum value from a given number of verbosity flags
    pub fn from_verbosity_flag_count(level: u8) -> Self {
        match level {
            0 => LogLevel::Error,
            1 => LogLevel::Warn,
            2 => LogLevel::Info,
            3 => LogLevel::Debug,
            _ => LogLevel::Trace,
        }
    }

    /// Applies log level to the system
    pub fn apply(&self) {
        log::set_max_level(LevelFilter::Trace);
        if env::var("RUST_LOG").is_err() {
            env::set_var("RUST_LOG", self.to_string());
        }
        env_logger::init();
    }
}

/// Marker trait that can be implemented for data structures used by `Clap` or
/// by any other form of API handling.
pub trait Exec {
    /// Runtime context data type, that is provided for execution context.
    type Client: Sized;
    /// Error type that may result from the execution
    type Error: std::error::Error;
    /// Main execution routine
    fn exec(self, client: &mut Self::Client) -> Result<(), Self::Error>;
}

/// Sets up command-line environment by assigning verbosity level to the logger
/// and replacing `{data_dir}` and `pat.0` components with the provided
/// `data_dir` and `pat.1` values. Also replaces the same patterns in the list
/// of service endpoints, if IPC files are used.
///
/// Creates `data_dir`, if it does not exist.
///
/// # Panics
///
/// Panics if the `data_dir` can't be created.
pub fn shell_setup<'endpoints>(verbosity: u8, data_dir: &mut PathBuf, pat: &[(&str, String)]) {
    LogLevel::from_verbosity_flag_count(verbosity).apply();

    let mut data_dir_s = data_dir.display().to_string();
    for (from, to) in pat {
        data_dir_s = data_dir_s.replace(from, to);
    }
    *data_dir = PathBuf::from(shellexpand::tilde(&data_dir_s).to_string());
    let data_dir_s = data_dir.display().to_string();

    fs::create_dir_all(&data_dir)
        .unwrap_or_else(|_| panic!("Unable to access data directory '{}'", data_dir.display()));
}

pub fn shell_expand_dir(path: &mut String, data_dir: &str, pat: &[(&str, String)]) {
    *path = path.replace("{data_dir}", data_dir);
    *path = shellexpand::tilde(path).to_string();
    for (from, to) in pat {
        *path = path.replace(from, to);
    }
}
