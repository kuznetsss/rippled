//! Accessor helpers for `BasicConfig` and `Section`.
//!
//! These methods keep the query code in `mod.rs` terse: one method call per
//! field rather than a raw `HashMap` lookup + parse chain every time.

use std::str::FromStr;

use crate::error::ParseError;
use crate::ini::parser::{BasicConfig, Section};

// ---------------------------------------------------------------------------
// BasicConfig helpers
// ---------------------------------------------------------------------------

/// Extension trait (implemented as inherent methods via a local wrapper) on
/// `BasicConfig`.  We add methods directly via `impl` since `BasicConfig` is a
/// type alias defined in this crate.
pub trait BasicConfigExt {
    /// Collect value-lines from section `name` into a `Vec<String>`.
    /// Returns an empty Vec if the section is absent.
    fn values_of(&self, name: &str) -> Vec<String>;

    /// Return the scalar value for a single-value section (value-lines only).
    /// For multi-line sections the lines are concatenated (no separator).
    /// Returns `None` if the section is absent or has no value-lines.
    fn scalar(&self, name: &str) -> Option<String>;

    /// Return the scalar value and parse it as `T`.
    fn scalar_parse<T: FromStr>(&self, name: &str) -> Result<Option<T>, ParseError>
    where
        T::Err: std::fmt::Display;

    /// Return the scalar value and parse it with the C++ bool dialect.
    fn scalar_bool(&self, name: &str) -> Result<Option<bool>, ParseError>;
}

impl BasicConfigExt for BasicConfig {
    fn values_of(&self, name: &str) -> Vec<String> {
        match self.get(name) {
            Some(sec) => sec.values.clone(),
            None => Vec::new(),
        }
    }

    fn scalar(&self, name: &str) -> Option<String> {
        let sec = self.get(name)?;
        if sec.values.is_empty() {
            None
        } else if sec.values.len() == 1 {
            Some(sec.values[0].clone())
        } else {
            Some(sec.values.concat())
        }
    }

    fn scalar_parse<T: FromStr>(&self, name: &str) -> Result<Option<T>, ParseError>
    where
        T::Err: std::fmt::Display,
    {
        match self.scalar(name) {
            None => Ok(None),
            Some(raw) => raw
                .parse::<T>()
                .map(Some)
                .map_err(|e| ParseError::Ini(format!("cannot parse [{name}] value '{raw}': {e}"))),
        }
    }

    fn scalar_bool(&self, name: &str) -> Result<Option<bool>, ParseError> {
        match self.scalar(name) {
            None => Ok(None),
            Some(raw) => parse_bool_compat(&raw)
                .map(Some)
                .ok_or_else(|| {
                    ParseError::Ini(format!(
                        "cannot parse [{name}] value '{raw}' as bool"
                    ))
                }),
        }
    }
}

// ---------------------------------------------------------------------------
// Section helpers
// ---------------------------------------------------------------------------

pub trait SectionExt {
    /// Return a borrowed `&str` for `key`, or `None`.
    fn get_str(&self, key: &str) -> Option<&str>;

    /// Return a cloned `String` for `key`, or `None`.
    fn get_string(&self, key: &str) -> Option<String>;

    /// Parse the value for `key` as `T`, or `None` if absent.
    fn get_parse<T: FromStr>(
        &self,
        key: &str,
        section: &str,
    ) -> Result<Option<T>, ParseError>
    where
        T::Err: std::fmt::Display;

    /// Parse the value for `key` with the C++ bool dialect.
    fn get_bool(&self, key: &str, section: &str) -> Result<Option<bool>, ParseError>;

    /// Parse the value for `key` with the C++ bool dialect (1/0 only; no
    /// lexical-cast aliases).  Used by `[ledger_tx_tables].use_tx_tables` which
    /// C++ reads via `getIfExists<bool>` (int-coerced path).
    fn get_bool_int_compat(&self, key: &str, section: &str) -> Result<Option<bool>, ParseError>;

    /// Comma-split the value for `key`; each token is trimmed.  Returns `None`
    /// if the key is absent (not an empty Vec).
    fn comma_split(&self, key: &str) -> Option<Vec<String>>;

    /// Comma-split the value for `key` and lowercase each token.
    fn comma_split_lowercase(&self, key: &str) -> Option<Vec<String>>;

    /// Verify that every key in `self.lookup` is in `allowed`.  Returns an
    /// error naming the first unexpected key.
    fn require_only_keys(&self, allowed: &[&str], section: &str) -> Result<(), ParseError>;
}

impl SectionExt for Section {
    fn get_str(&self, key: &str) -> Option<&str> {
        self.lookup.get(key).map(String::as_str)
    }

    fn get_string(&self, key: &str) -> Option<String> {
        self.lookup.get(key).cloned()
    }

    fn get_parse<T: FromStr>(
        &self,
        key: &str,
        section: &str,
    ) -> Result<Option<T>, ParseError>
    where
        T::Err: std::fmt::Display,
    {
        match self.lookup.get(key) {
            None => Ok(None),
            Some(raw) => raw
                .parse::<T>()
                .map(Some)
                .map_err(|e| {
                    ParseError::Ini(format!(
                        "cannot parse [{section}].{key} value '{raw}': {e}"
                    ))
                }),
        }
    }

    fn get_bool(&self, key: &str, section: &str) -> Result<Option<bool>, ParseError> {
        match self.lookup.get(key) {
            None => Ok(None),
            Some(raw) => parse_bool_compat(raw)
                .map(Some)
                .ok_or_else(|| {
                    ParseError::Ini(format!(
                        "cannot parse [{section}].{key} value '{raw}' as bool"
                    ))
                }),
        }
    }

    fn get_bool_int_compat(&self, key: &str, section: &str) -> Result<Option<bool>, ParseError> {
        match self.lookup.get(key) {
            None => Ok(None),
            Some(raw) => match raw.trim() {
                "1" => Ok(Some(true)),
                "0" => Ok(Some(false)),
                _ => parse_bool_compat(raw).map(Some).ok_or_else(|| {
                    ParseError::Ini(format!(
                        "cannot parse [{section}].{key} value '{raw}' as int-bool"
                    ))
                }),
            },
        }
    }

    fn comma_split(&self, key: &str) -> Option<Vec<String>> {
        self.lookup.get(key).map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
    }

    fn comma_split_lowercase(&self, key: &str) -> Option<Vec<String>> {
        self.lookup.get(key).map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|t| t.to_ascii_lowercase())
                .collect()
        })
    }

    fn require_only_keys(&self, allowed: &[&str], section: &str) -> Result<(), ParseError> {
        for key in self.lookup.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(ParseError::Ini(format!(
                    "unexpected key '{key}' in [{section}]"
                )));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Stand-alone helpers
// ---------------------------------------------------------------------------

/// Parse a C++-compatible boolean.
/// Accepts `true/false/yes/no/on/off/1/0` case-insensitively.
pub fn parse_bool_compat(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// Parse a polymorphic u32: numeric first, then named aliases.
///
/// `aliases` is a slice of `(name, value)` pairs; names are compared
/// case-insensitively.
pub fn parse_polymorphic_u32(
    s: &str,
    aliases: &[(&str, u32)],
    context: &str,
) -> Result<u32, ParseError> {
    if let Ok(n) = s.parse::<u32>() {
        return Ok(n);
    }
    let lower = s.to_ascii_lowercase();
    for &(alias, val) in aliases {
        if lower == alias {
            return Ok(val);
        }
    }
    Err(ParseError::Ini(format!(
        "invalid polymorphic value '{s}' for {context}"
    )))
}

/// Parse a polymorphic u8: numeric first, then named aliases.
pub fn parse_polymorphic_u8(
    s: &str,
    aliases: &[(&str, u8)],
    context: &str,
) -> Result<u8, ParseError> {
    if let Ok(n) = s.parse::<u8>() {
        return Ok(n);
    }
    let lower = s.to_ascii_lowercase();
    for &(alias, val) in aliases {
        if lower == alias {
            return Ok(val);
        }
    }
    Err(ParseError::Ini(format!(
        "invalid polymorphic value '{s}' for {context}"
    )))
}
