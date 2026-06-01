use std::fs;
use std::path::Path;

use crate::config_builder::ConfigBuilder;
use crate::error::ParseError;
use crate::ini::parse_ini;
use crate::schema::Config;

/// Recognised config file formats.
///
/// Used by [`parse_from_str`] callers who supply a format explicitly and by
/// [`parse_from_file`] which detects the format from the file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Toml,
    Ini,
}

impl ConfigFormat {
    /// Determine format from `path`'s extension (case-insensitive).
    ///
    /// * `.toml`                    → `Toml`
    /// * `.ini`, `.cfg`, `.txt`     → `Ini`
    /// * anything else / missing    → `Err(ParseError::UnsupportedFormat)`
    pub fn from_path(path: &Path) -> Result<Self, ParseError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "toml" => Ok(Self::Toml),
            "ini" | "cfg" | "txt" => Ok(Self::Ini),
            other => Err(ParseError::UnsupportedFormat(other.to_owned())),
        }
    }
}

/// Warnings emitted when parsing a legacy INI file.
#[derive(Debug, Default, Clone)]
pub struct IniWarnings {
    /// `true` when any section contained trailing `# comments` (after
    /// comment-stripping), matching the behaviour of `BasicConfig::hadTrailingComments()`.
    pub had_trailing_comments: bool,
}

// ---------------------------------------------------------------------------
// Pure parsers — no normalise, no validate.  Called only from parse_from_str.
// ---------------------------------------------------------------------------

/// Parse a `Config` from an in-memory TOML document.  Pure parse only —
/// does **not** run normalise or validate.
pub(crate) fn parse_from_toml_str(s: &str) -> Result<Config, ParseError> {
    Ok(toml::from_str(s)?)
}

/// Parse a `Config` from an in-memory legacy INI document.  Pure parse only —
/// does **not** run normalise or validate.
///
/// Returns `(Config, IniWarnings)` on success.
pub(crate) fn parse_from_ini_str(s: &str) -> Result<(Config, IniWarnings), ParseError> {
    let bc = parse_ini(s);
    let had_trailing_comments = bc.values().any(|sec| sec.had_trailing_comments);
    let config: Config = crate::ini::from_basic_config(&bc)?;
    Ok((
        config,
        IniWarnings {
            had_trailing_comments,
        },
    ))
}

// ---------------------------------------------------------------------------
// Public parse entry points — return a ConfigBuilder (not yet finalized)
// ---------------------------------------------------------------------------

/// Parse a `Config` from an in-memory document of the given format.
///
/// Returns a [`ConfigBuilder`] that the caller can configure with CLI flags
/// before calling [`ConfigBuilder::finalize`] to run normalize + validate.
///
/// Relative paths in the config (e.g. `database_path`, `debug_logfile`,
/// `validators_file`) are resolved against the builder's `config_dir` at
/// finalize time; leave `config_dir` unset (via this function) for in-memory
/// configs that don't reference relative paths.
///
/// For TOML, `had_trailing_comments` is always `false`.
pub fn parse_from_str(content: &str, format: ConfigFormat) -> Result<ConfigBuilder, ParseError> {
    let (cfg, warnings) = match format {
        ConfigFormat::Toml => (parse_from_toml_str(content)?, IniWarnings::default()),
        ConfigFormat::Ini => parse_from_ini_str(content)?,
    };
    Ok(ConfigBuilder::new(cfg, None, warnings))
}

/// Read a config file from disk and return a [`ConfigBuilder`].
///
/// The format is detected from the file extension (case-insensitive):
///
/// * `.toml`                  → TOML parser
/// * `.ini`, `.cfg`, `.txt`   → legacy INI parser
/// * anything else / missing  → [`ParseError::UnsupportedFormat`]
///
/// The builder's `config_dir` is set to the file's parent directory so
/// relative paths in the config resolve correctly at finalize time.
///
/// For TOML files `had_trailing_comments` is always `false`.
pub fn parse_from_file<P: AsRef<Path>>(path: P) -> Result<ConfigBuilder, ParseError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)?;
    let format = ConfigFormat::from_path(path)?;
    let config_dir = path.parent().map(Path::to_path_buf);
    let (cfg, warnings) = match format {
        ConfigFormat::Toml => (parse_from_toml_str(&contents)?, IniWarnings::default()),
        ConfigFormat::Ini => parse_from_ini_str(&contents)?,
    };
    Ok(ConfigBuilder::new(cfg, config_dir, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_from_str_toml_minimal() {
        let builder = parse_from_str("network_quorum = 3", ConfigFormat::Toml).unwrap();
        let (cfg, _) = builder.finalize().unwrap();
        assert_eq!(cfg.network_quorum, Some(3));
    }

    #[test]
    fn parse_from_str_surfaces_parse_errors() {
        let err = parse_from_str("not_a_real_key = 1", ConfigFormat::Toml).unwrap_err();
        assert!(matches!(err, ParseError::Toml(_)), "got {err:?}");
    }
}
