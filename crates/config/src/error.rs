//! Error type for config parsing and the parse-result wrapper exposed across
//! the FFI boundary.

use std::io;

use crate::schema::Config;
use crate::IniWarnings;

/// Errors returned by the config parsers.
#[derive(Debug)]
pub enum ParseError {
    Io(io::Error),
    Toml(toml::de::Error),
    Ini(String),
    /// File extension didn't match any known config format.
    UnsupportedFormat(String),
    /// A value appears in both the main config and the validators file when
    /// strict merging is in effect.
    DuplicateValue(String),
}

/// Errors returned by [`Config::bootstrap`].
///
/// A separate type from `ParseError` so the C++ bridge can surface it
/// distinctly.  Currently only wraps I/O failures from directory creation.
#[derive(Debug)]
pub enum BootstrapError {
    Io(io::Error),
}

impl std::fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootstrapError::Io(e) => write!(f, "bootstrap I/O error: {e}"),
        }
    }
}

impl std::error::Error for BootstrapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BootstrapError::Io(e) => Some(e),
        }
    }
}

impl From<io::Error> for BootstrapError {
    fn from(e: io::Error) -> Self {
        BootstrapError::Io(e)
    }
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
            ParseError::DuplicateValue(msg) => write!(f, "duplicate validator entry: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParseError::Io(e) => Some(e),
            ParseError::Toml(e) => Some(e),
            ParseError::Ini(_) | ParseError::UnsupportedFormat(_) | ParseError::DuplicateValue(_) => None,
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
/// `had_trailing_comments()` returns `true` when the parsed file was an INI
/// file that contained trailing `#` comments (post-stripping).  Always
/// `false` for TOML files and error results.
///
/// The internal `Option` exists because cxx doesn't allow `Box<Self>` as a
/// receiver: `value()` has to take `&mut self`, so the move-out is done with
/// `Option::take`. Calling `value()` a second time throws (the slot is empty).
pub struct ParseOutcome {
    inner: Option<Result<Box<Config>, ParseError>>,
    warnings: IniWarnings,
}

impl ParseOutcome {
    /// Wrap a TOML parser's `Result` into an outcome handle (no warnings).
    pub fn from_toml_result(result: Result<Config, ParseError>) -> Box<Self> {
        Box::new(Self {
            inner: Some(result.map(Box::new)),
            warnings: IniWarnings::default(),
        })
    }

    /// Wrap an INI parser's `Result` (includes warnings) into an outcome handle.
    pub fn from_ini_result(result: Result<(Config, IniWarnings), ParseError>) -> Box<Self> {
        let (inner, warnings) = match result {
            Ok((cfg, w)) => (Ok(Box::new(cfg)), w),
            Err(e) => (Err(e), IniWarnings::default()),
        };
        Box::new(Self {
            inner: Some(inner),
            warnings,
        })
    }

    pub fn has_value(&self) -> bool {
        matches!(&self.inner, Some(Ok(_)))
    }

    pub fn has_error(&self) -> bool {
        matches!(&self.inner, Some(Err(_)))
    }

    /// Returns `true` if the parse succeeded and the source was an INI file
    /// that contained trailing comments.  Always `false` for TOML and errors.
    /// Survives `value()` consumption.
    pub fn had_trailing_comments(&self) -> bool {
        self.warnings.had_trailing_comments
    }

    /// Move the parsed `Config` out of the outcome. Throws (via cxx) if
    /// `has_value()` is false, or if `value()` was already called once.
    pub fn value(&mut self) -> Result<Box<Config>, String> {
        match self.inner.take() {
            Some(Ok(cfg)) => Ok(cfg),
            Some(Err(e)) => {
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
