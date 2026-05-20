use serde::{Deserialize, Serialize};

/// Safety level for SQLite — matches `safety_level` key values.
/// Matched case-insensitively per analysis §7 #4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SqliteSafety {
    High,
    Low,
}

/// SQLite journal mode values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SqliteJournalMode {
    Delete,
    Truncate,
    Persist,
    Memory,
    Wal,
    Off,
}

/// SQLite `synchronous` pragma values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SqliteSynchronous {
    Off,
    Normal,
    Full,
    Extra,
}

/// SQLite `temp_store` pragma values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SqliteTempStore {
    Default,
    File,
    Memory,
}

/// The SQLite tuning mode.
///
/// `Safety` and `Tuning` are mutually exclusive: `safety_level` cannot coexist
/// with `journal_mode`, `synchronous`, or `temp_store` (analysis §5 / design §3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SqliteMode {
    /// Use a named safety level preset.
    Safety { level: SqliteSafety },
    /// Explicit tuning — individual pragma overrides.
    Tuning {
        journal_mode: Option<SqliteJournalMode>,
        synchronous: Option<SqliteSynchronous>,
        temp_store: Option<SqliteTempStore>,
        /// Must be a power of 2 in 512..=65536. Default 4096.
        page_size: u32,
    },
    /// Neither block was set — use SQLite defaults.
    Default,
}

impl Default for SqliteMode {
    fn default() -> Self {
        SqliteMode::Default
    }
}

/// Configuration for the `[sqlite]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SqliteConfig {
    pub mode: SqliteMode,
    /// Journal size limit in bytes. Default 1_582_080.
    pub journal_size_limit: i64,
}

impl Default for SqliteConfig {
    fn default() -> Self {
        SqliteConfig {
            mode: SqliteMode::Default,
            journal_size_limit: 1_582_080,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_default_values() {
        let c = SqliteConfig::default();
        assert_eq!(c.journal_size_limit, 1_582_080);
        assert_eq!(c.mode, SqliteMode::Default);
    }

    #[test]
    fn sqlite_mode_default_is_default_variant() {
        assert_eq!(SqliteMode::default(), SqliteMode::Default);
    }

    #[test]
    fn sqlite_default_passes_strict_validation() {
        SqliteConfig::default().validate_strict().expect("default should be valid");
    }

    #[test]
    fn sqlite_tuning_page_size_valid_512() {
        let c = SqliteConfig {
            mode: SqliteMode::Tuning {
                journal_mode: None,
                synchronous: None,
                temp_store: None,
                page_size: 512,
            },
            journal_size_limit: 1_582_080,
        };
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn sqlite_tuning_page_size_valid_4096() {
        let c = SqliteConfig {
            mode: SqliteMode::Tuning {
                journal_mode: None,
                synchronous: None,
                temp_store: None,
                page_size: 4096,
            },
            journal_size_limit: 1_582_080,
        };
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn sqlite_tuning_page_size_valid_65536() {
        let c = SqliteConfig {
            mode: SqliteMode::Tuning {
                journal_mode: None,
                synchronous: None,
                temp_store: None,
                page_size: 65536,
            },
            journal_size_limit: 1_582_080,
        };
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn sqlite_tuning_page_size_too_small() {
        let c = SqliteConfig {
            mode: SqliteMode::Tuning {
                journal_mode: None,
                synchronous: None,
                temp_store: None,
                page_size: 256,
            },
            journal_size_limit: 1_582_080,
        };
        let err = c.validate_strict().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("page_size"), "got: {msg}");
    }

    #[test]
    fn sqlite_tuning_page_size_too_large() {
        let c = SqliteConfig {
            mode: SqliteMode::Tuning {
                journal_mode: None,
                synchronous: None,
                temp_store: None,
                page_size: 131072,
            },
            journal_size_limit: 1_582_080,
        };
        assert!(c.validate_strict().is_err());
    }

    #[test]
    fn sqlite_tuning_page_size_not_power_of_two() {
        let c = SqliteConfig {
            mode: SqliteMode::Tuning {
                journal_mode: None,
                synchronous: None,
                temp_store: None,
                page_size: 3000, // in range but not power of 2
            },
            journal_size_limit: 1_582_080,
        };
        assert!(c.validate_strict().is_err());
    }

    #[test]
    fn sqlite_safety_mode_passes_strict_validation() {
        let c = SqliteConfig {
            mode: SqliteMode::Safety { level: SqliteSafety::High },
            journal_size_limit: 1_582_080,
        };
        assert!(c.validate_strict().is_ok());
    }

    #[test]
    fn sqlite_safety_low_passes_strict_validation() {
        let c = SqliteConfig {
            mode: SqliteMode::Safety { level: SqliteSafety::Low },
            journal_size_limit: 1_582_080,
        };
        assert!(c.validate_strict().is_ok());
    }
}
