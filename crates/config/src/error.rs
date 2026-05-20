use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

/// Identifies which config format an error originated from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Format {
    Ini,
    Toml,
}

/// A span (location) within a config source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpan {
    pub line: u32,
    pub col_start: u32,
    pub col_end: u32,
}

/// Sub-error type for lexer failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub reason: String,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason)
    }
}

/// All the ways config parsing and bootstrap can fail.
#[derive(Debug, Clone)]
pub enum ConfigErrorKind {
    Lex {
        reason: LexError,
    },
    /// Unknown top-level section (TOML strict only).
    UnknownSection {
        name: String,
        format: Format,
    },
    /// Unknown key inside a known section; `suggestion` is a did-you-mean hint.
    UnknownKey {
        section: String,
        key: String,
        suggestion: Option<String>,
    },
    /// A field value failed to parse according to the expected grammar.
    Grammar {
        what: &'static str,
        value: String,
        reason: String,
    },
    /// A numeric value falls outside the allowed range.
    OutOfRange {
        field: String,
        value: i64,
        min: Option<i64>,
        max: Option<i64>,
    },
    /// Two mutually exclusive fields were both set.
    MutualExclusion {
        first: String,
        second: String,
    },
    /// A `[port.<name>]` table has no matching entry in `server.ports` (TOML strict).
    OrphanPortTable {
        name: String,
    },
    /// The same validator section appears in both the main config and `validators.txt`
    /// (TOML strict).
    ValidatorsFileOverlap {
        section: String,
    },
    /// Catch-all for cross-section validation failures (§5 validators).
    Cross {
        what: String,
    },
    /// An I/O error while reading a config file or the data directory.
    Io {
        path: PathBuf,
        /// Stored behind `Arc` so `ConfigErrorKind` can implement `Clone`.
        source: Arc<io::Error>,
    },
}

impl std::fmt::Display for ConfigErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigErrorKind::Lex { reason } => write!(f, "lex error: {}", reason),
            ConfigErrorKind::UnknownSection { name, .. } => {
                write!(f, "unknown section `{name}`")
            }
            ConfigErrorKind::UnknownKey {
                section,
                key,
                suggestion,
            } => {
                write!(f, "unknown key `{key}` in section [{section}]")?;
                if let Some(s) = suggestion {
                    write!(f, "\n  = note: did you mean `{s}`?")?;
                }
                Ok(())
            }
            ConfigErrorKind::Grammar { what, value, reason } => {
                write!(f, "invalid {what} value `{value}`: {reason}")
            }
            ConfigErrorKind::OutOfRange {
                field,
                value,
                min,
                max,
            } => {
                write!(f, "value {value} for `{field}` is out of range")?;
                match (min, max) {
                    (Some(lo), Some(hi)) => write!(f, " (expected {lo}..={hi})")?,
                    (Some(lo), None) => write!(f, " (expected >= {lo})")?,
                    (None, Some(hi)) => write!(f, " (expected <= {hi})")?,
                    (None, None) => {}
                }
                Ok(())
            }
            ConfigErrorKind::MutualExclusion { first, second } => {
                write!(f, "`{first}` and `{second}` are mutually exclusive")
            }
            ConfigErrorKind::OrphanPortTable { name } => {
                write!(f, "port table `{name}` has no matching entry in server.ports")
            }
            ConfigErrorKind::ValidatorsFileOverlap { section } => {
                write!(
                    f,
                    "section `{section}` appears in both the main config and validators.txt"
                )
            }
            ConfigErrorKind::Cross { what } => write!(f, "validation error: {what}"),
            ConfigErrorKind::Io { path, source } => {
                write!(f, "I/O error reading `{}`: {source}", path.display())
            }
        }
    }
}

/// A config parse or bootstrap error, with optional source location.
#[derive(Debug, Clone, Error)]
#[error("{}", self.display_message())]
pub struct ConfigError {
    pub kind: ConfigErrorKind,
    pub span: Option<SourceSpan>,
    pub source_file: Option<PathBuf>,
}

impl ConfigError {
    fn display_message(&self) -> String {
        self.message()
    }

    /// Produce the human-readable error message (also used by FFI to return a plain string).
    pub fn message(&self) -> String {
        match (&self.source_file, &self.span) {
            (Some(path), Some(span)) => format!(
                "config error at {}:{}:{}: {}",
                path.display(),
                span.line,
                span.col_start,
                self.kind
            ),
            (Some(path), None) => format!("config error in {}: {}", path.display(), self.kind),
            _ => format!("config error: {}", self.kind),
        }
    }

    // ---- helper constructors used by parser code ----

    pub fn lex(reason: impl Into<String>) -> Self {
        ConfigError {
            kind: ConfigErrorKind::Lex {
                reason: LexError {
                    reason: reason.into(),
                },
            },
            span: None,
            source_file: None,
        }
    }

    pub fn unknown_section(name: impl Into<String>, format: Format) -> Self {
        ConfigError {
            kind: ConfigErrorKind::UnknownSection {
                name: name.into(),
                format,
            },
            span: None,
            source_file: None,
        }
    }

    pub fn unknown_key(
        section: impl Into<String>,
        key: impl Into<String>,
        suggestion: Option<String>,
    ) -> Self {
        ConfigError {
            kind: ConfigErrorKind::UnknownKey {
                section: section.into(),
                key: key.into(),
                suggestion,
            },
            span: None,
            source_file: None,
        }
    }

    pub fn grammar(what: &'static str, value: impl Into<String>, reason: impl Into<String>) -> Self {
        ConfigError {
            kind: ConfigErrorKind::Grammar {
                what,
                value: value.into(),
                reason: reason.into(),
            },
            span: None,
            source_file: None,
        }
    }

    pub fn out_of_range(
        field: impl Into<String>,
        value: i64,
        min: Option<i64>,
        max: Option<i64>,
    ) -> Self {
        ConfigError {
            kind: ConfigErrorKind::OutOfRange {
                field: field.into(),
                value,
                min,
                max,
            },
            span: None,
            source_file: None,
        }
    }

    pub fn mutual_exclusion(first: impl Into<String>, second: impl Into<String>) -> Self {
        ConfigError {
            kind: ConfigErrorKind::MutualExclusion {
                first: first.into(),
                second: second.into(),
            },
            span: None,
            source_file: None,
        }
    }

    pub fn orphan_port_table(name: impl Into<String>) -> Self {
        ConfigError {
            kind: ConfigErrorKind::OrphanPortTable { name: name.into() },
            span: None,
            source_file: None,
        }
    }

    pub fn validators_file_overlap(section: impl Into<String>) -> Self {
        ConfigError {
            kind: ConfigErrorKind::ValidatorsFileOverlap {
                section: section.into(),
            },
            span: None,
            source_file: None,
        }
    }

    pub fn cross(what: impl Into<String>) -> Self {
        ConfigError {
            kind: ConfigErrorKind::Cross { what: what.into() },
            span: None,
            source_file: None,
        }
    }

    pub fn io(path: PathBuf, source: io::Error) -> Self {
        ConfigError {
            kind: ConfigErrorKind::Io {
                path,
                source: Arc::new(source),
            },
            span: None,
            source_file: None,
        }
    }

    /// Attach a source span to this error.
    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    /// Attach a source file path to this error.
    pub fn with_file(mut self, path: PathBuf) -> Self {
        self.source_file = Some(path);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Display formatting ----

    #[test]
    fn lex_display() {
        let e = ConfigError::lex("unexpected character `@`");
        let msg = e.kind.to_string();
        assert!(msg.contains("lex error"), "got: {msg}");
        assert!(msg.contains("unexpected character"), "got: {msg}");
    }

    #[test]
    fn unknown_section_display() {
        let e = ConfigError::unknown_section("foobar", Format::Toml);
        let msg = e.kind.to_string();
        assert!(msg.contains("foobar"), "got: {msg}");
        assert!(msg.contains("unknown section"), "got: {msg}");
    }

    #[test]
    fn unknown_key_without_suggestion_display() {
        let e = ConfigError::unknown_key("overlay", "max_unkown_time", None);
        let msg = e.kind.to_string();
        assert!(msg.contains("max_unkown_time"), "got: {msg}");
        assert!(msg.contains("overlay"), "got: {msg}");
        assert!(!msg.contains("did you mean"), "got: {msg}");
    }

    #[test]
    fn unknown_key_with_suggestion_display() {
        let e = ConfigError::unknown_key(
            "overlay",
            "max_unkown_time",
            Some("max_unknown_time".to_owned()),
        );
        let msg = e.kind.to_string();
        assert!(msg.contains("did you mean"), "got: {msg}");
        assert!(msg.contains("max_unknown_time"), "got: {msg}");
    }

    #[test]
    fn grammar_display() {
        let e = ConfigError::grammar("HostPort", "bad::value:99999", "invalid port number");
        let msg = e.kind.to_string();
        assert!(msg.contains("HostPort"), "got: {msg}");
        assert!(msg.contains("bad::value:99999"), "got: {msg}");
        assert!(msg.contains("invalid port number"), "got: {msg}");
    }

    #[test]
    fn out_of_range_with_both_bounds_display() {
        let e = ConfigError::out_of_range("overlay.max_unknown_time", 9999, Some(300), Some(1800));
        let msg = e.kind.to_string();
        assert!(msg.contains("9999"), "got: {msg}");
        assert!(msg.contains("overlay.max_unknown_time"), "got: {msg}");
        assert!(msg.contains("300"), "got: {msg}");
        assert!(msg.contains("1800"), "got: {msg}");
    }

    #[test]
    fn out_of_range_lower_only_display() {
        let e = ConfigError::out_of_range("some.field", 0, Some(1), None);
        let msg = e.kind.to_string();
        assert!(msg.contains(">= 1"), "got: {msg}");
    }

    #[test]
    fn out_of_range_upper_only_display() {
        let e = ConfigError::out_of_range("some.field", 100, None, Some(50));
        let msg = e.kind.to_string();
        assert!(msg.contains("<= 50"), "got: {msg}");
    }

    #[test]
    fn out_of_range_no_bounds_display() {
        let e = ConfigError::out_of_range("some.field", 42, None, None);
        let msg = e.kind.to_string();
        assert!(msg.contains("42"), "got: {msg}");
        assert!(msg.contains("out of range"), "got: {msg}");
    }

    #[test]
    fn mutual_exclusion_display() {
        let e = ConfigError::mutual_exclusion("safety_level", "journal_mode");
        let msg = e.kind.to_string();
        assert!(msg.contains("safety_level"), "got: {msg}");
        assert!(msg.contains("journal_mode"), "got: {msg}");
        assert!(msg.contains("mutually exclusive"), "got: {msg}");
    }

    #[test]
    fn orphan_port_table_display() {
        let e = ConfigError::orphan_port_table("peer");
        let msg = e.kind.to_string();
        assert!(msg.contains("peer"), "got: {msg}");
        assert!(msg.contains("server.ports"), "got: {msg}");
    }

    #[test]
    fn validators_file_overlap_display() {
        let e = ConfigError::validators_file_overlap("validator_list_keys");
        let msg = e.kind.to_string();
        assert!(msg.contains("validator_list_keys"), "got: {msg}");
        assert!(msg.contains("validators.txt"), "got: {msg}");
    }

    #[test]
    fn cross_display() {
        let e = ConfigError::cross("maximum_txn_in_ledger must be >= minimum_txn_in_ledger");
        let msg = e.kind.to_string();
        assert!(msg.contains("maximum_txn_in_ledger"), "got: {msg}");
    }

    #[test]
    fn io_display() {
        let e = ConfigError::io(
            PathBuf::from("/etc/xrpld.cfg"),
            io::Error::new(io::ErrorKind::NotFound, "file not found"),
        );
        let msg = e.kind.to_string();
        assert!(msg.contains("/etc/xrpld.cfg"), "got: {msg}");
        assert!(msg.contains("I/O error"), "got: {msg}");
    }

    // ---- Constructor fields ----

    #[test]
    fn grammar_constructor_populates_fields() {
        let e = ConfigError::grammar("MyType", "bad_value", "it is bad");
        match &e.kind {
            ConfigErrorKind::Grammar { what, value, reason } => {
                assert_eq!(*what, "MyType");
                assert_eq!(value, "bad_value");
                assert_eq!(reason, "it is bad");
            }
            _ => panic!("wrong kind"),
        }
        assert!(e.span.is_none());
        assert!(e.source_file.is_none());
    }

    #[test]
    fn out_of_range_constructor_populates_fields() {
        let e = ConfigError::out_of_range("field.x", 42, Some(1), Some(100));
        match &e.kind {
            ConfigErrorKind::OutOfRange { field, value, min, max } => {
                assert_eq!(field, "field.x");
                assert_eq!(*value, 42);
                assert_eq!(*min, Some(1));
                assert_eq!(*max, Some(100));
            }
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn message_equals_display() {
        let e = ConfigError::grammar("T", "v", "r");
        assert_eq!(e.message(), format!("config error: {}", e.kind));
    }

    #[test]
    fn message_with_span_and_file() {
        let e = ConfigError::grammar("T", "v", "r")
            .with_span(SourceSpan { line: 10, col_start: 5, col_end: 10 })
            .with_file(PathBuf::from("/etc/xrpld.cfg"));
        let msg = e.message();
        assert!(msg.contains("/etc/xrpld.cfg"), "got: {msg}");
        assert!(msg.contains("10"), "got: {msg}");
        assert!(msg.contains("5"), "got: {msg}");
    }

    #[test]
    fn message_with_file_no_span() {
        let e = ConfigError::grammar("T", "v", "r")
            .with_file(PathBuf::from("/etc/xrpld.cfg"));
        let msg = e.message();
        assert!(msg.contains("/etc/xrpld.cfg"), "got: {msg}");
        // no line:col since span is None
        assert!(!msg.contains(":0:"), "got: {msg}");
    }

    #[test]
    fn message_no_location() {
        let e = ConfigError::grammar("T", "v", "r");
        let msg = e.message();
        assert!(msg.starts_with("config error: "), "got: {msg}");
    }
}
