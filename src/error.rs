use std::backtrace::Backtrace;
use std::path::Path;

use regex::Regex;
use thiserror::Error as ThisError;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(ThisError, Debug)]
#[error(transparent)]
pub enum SerializedError {
    Toml(#[from] toml::ser::Error),
    Json(#[from] serde_json::error::Error),
}

#[derive(ThisError, Debug)]
#[error(transparent)]
pub enum DeserializedError {
    Toml(#[from] toml::de::Error),
    Json(#[from] serde_json::error::Error),
    #[error("{unmatched:?} unmatched regex: \"{regex}\"")]
    Regex {
        unmatched: String,
        regex: String,
    },
    ParseInt(#[from] std::num::ParseIntError),
    AddrParse(#[from] std::net::AddrParseError),
    NetworkParse(#[from] cidr::errors::NetworkParseError),
    ParseUrl(#[from] url::ParseError),
    #[error("{0}")]
    Custom(String),
}

impl DeserializedError {
    pub fn regex(unmatched: String, regex: &Regex) -> Self {
        Self::Regex {
            unmatched,
            regex: regex.to_string(),
        }
    }
    pub fn custom(reason: &str) -> Self {
        Self::Custom(reason.into())
    }
}

#[derive(ThisError, Debug)]
pub enum ReqwestError {
    #[error("Reqwest build error, source: {source}")]
    Build { source: reqwest::Error },
    #[error("Reqwest network error, source: {source}")]
    Network { source: reqwest::Error },
    #[error("Expected body {expected:?} not found in {body:?}")]
    BodyNoMatch { body: String, expected: String },
}

impl ReqwestError {
    pub fn build(err: reqwest::Error) -> Self {
        Self::Build { source: err }
    }
    pub fn network(err: reqwest::Error) -> Self {
        Self::Network { source: err }
    }
    pub fn body_no_match(body: String, expected: String) -> Self {
        Self::BodyNoMatch { body, expected }
    }
}

#[derive(ThisError, Debug)]
#[error(transparent)]
pub struct Error(pub Box<ErrorKind>);

impl<E> From<E> for Error
where
    ErrorKind: From<E>,
{
    fn from(err: E) -> Self {
        Error(Box::new(ErrorKind::from(err)))
    }
}

#[derive(ThisError, Debug)]
#[error(transparent)]
pub enum TokioError {
    Join(#[from] tokio::task::JoinError),
}

#[derive(ThisError, Debug)]
pub enum ErrorKind {
    #[error("IO error when processing file {path}\nCause: {source}\nBacktrace: {backtrace}")]
    Fs {
        source: std::io::Error,
        path: String,
        backtrace: Backtrace,
    },
    #[error("Process IO error\nCause: {source}\nBacktrace: {backtrace}")]
    Process {
        source: std::io::Error,
        backtrace: Backtrace,
    },
    #[error("Serialized error\nCause: {0}\nBacktrace: {1}")]
    Serialized(#[from] SerializedError, Backtrace),
    #[error("Deserialized error\nCause: {0}\nBacktrace: {1}")]
    Deserialized(#[from] DeserializedError, Backtrace),
    #[error("ReqwestError error\nCause: {0}\nBacktrace: {1}")]
    Reqwest(#[from] ReqwestError, Backtrace),
    #[error("JoinError error\nCause: {0}\nBacktrace: {1}")]
    Tokio(#[from] TokioError, Backtrace),
}

impl ErrorKind {
    pub fn fs<P: AsRef<Path>>(err: std::io::Error, path: P) -> Self {
        Self::Fs {
            source: err,
            path: format!("{:?}", path.as_ref()),
            backtrace: Backtrace::capture(),
        }
    }
    pub fn process(source: std::io::Error) -> Self {
        Self::Process {
            source,
            backtrace: Backtrace::capture(),
        }
    }
}
