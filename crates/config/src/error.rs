//! Error type for config parsing.

use std::io;

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
