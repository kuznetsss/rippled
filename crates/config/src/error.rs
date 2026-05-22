//! Error type for config parsing and the parse-result wrapper exposed across
//! the FFI boundary.

use std::io;

use crate::schema::Config;

/// Errors returned by the config parsers.
#[derive(Debug)]
pub enum ParseError {
    Io(io::Error),
    Toml(toml::de::Error),
    Ini(String),
    /// File extension didn't match any known config format.
    UnsupportedFormat(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Io(e) => write!(f, "I/O error: {e}"),
            ParseError::Toml(e) => write!(f, "TOML parse error: {e}"),
            ParseError::Ini(e) => write!(f, "INI parse error: {e}"),
            ParseError::UnsupportedFormat(ext) if ext.is_empty() => {
                write!(f, "unsupported config format: no file extension")
            }
            ParseError::UnsupportedFormat(ext) => {
                write!(f, "unsupported config format: .{ext}")
            }
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParseError::Io(e) => Some(e),
            ParseError::Toml(e) => Some(e),
            ParseError::Ini(_) | ParseError::UnsupportedFormat(_) => None,
        }
    }
}

impl From<io::Error> for ParseError {
    fn from(e: io::Error) -> Self {
        ParseError::Io(e)
    }
}

impl From<toml::de::Error> for ParseError {
    fn from(e: toml::de::Error) -> Self {
        ParseError::Toml(e)
    }
}

/// `std::expected`-shaped wrapper around the result of a parse call.
///
/// Exposed across the FFI boundary as an opaque type. The C++ side calls
/// `has_value()` / `has_error()` to discriminate, then `value()` or
/// `error()` to retrieve. Accessing the wrong arm throws — programmer error,
/// same semantics as `std::expected::value()` on an unexpected.
///
/// The internal `Option` exists because cxx doesn't allow `Box<Self>` as a
/// receiver: `value()` has to take `&mut self`, so the move-out is done with
/// `Option::take`. Calling `value()` a second time throws (the slot is empty).
pub struct ParseOutcome {
    inner: Option<Result<Box<Config>, ParseError>>,
}

impl ParseOutcome {
    /// Wrap a parser's `Result` into an outcome handle.
    pub fn from_result(result: Result<Config, ParseError>) -> Box<Self> {
        Box::new(Self {
            inner: Some(result.map(Box::new)),
        })
    }

    pub fn has_value(&self) -> bool {
        matches!(&self.inner, Some(Ok(_)))
    }

    pub fn has_error(&self) -> bool {
        matches!(&self.inner, Some(Err(_)))
    }

    /// Move the parsed `Config` out of the outcome. Throws (via cxx) if
    /// `has_value()` is false, or if `value()` was already called once.
    pub fn value(&mut self) -> Result<Box<Config>, String> {
        match self.inner.take() {
            Some(Ok(cfg)) => Ok(cfg),
            Some(Err(e)) => {
                // Put the error back so `error()` still works afterwards.
                self.inner = Some(Err(e));
                Err("ParseOutcome::value() called on an error result".into())
            }
            None => Err("ParseOutcome::value() called after value() already consumed it".into()),
        }
    }

    /// Return the error message. Throws (via cxx) if `has_error()` is false.
    pub fn error(&self) -> Result<String, String> {
        match &self.inner {
            Some(Err(e)) => Ok(e.to_string()),
            Some(Ok(_)) => Err("ParseOutcome::error() called on a successful result".into()),
            None => Err("ParseOutcome::error() called after value() consumed the result".into()),
        }
    }
}
