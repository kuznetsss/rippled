use std::fs;
use std::path::Path;

use crate::error::ParseError;
use crate::ini::parse_ini;
use crate::load_options::LoadOptions;
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
// Public parse entry points
// ---------------------------------------------------------------------------

/// Parse a `Config` from an in-memory document of the given format, then run
/// normalise and validate exactly once.
///
/// Relative paths in the config (e.g. `database_path`, `debug_logfile`,
/// `validators_file`) are resolved against `opts.config_dir`; leave it
/// unset for in-memory configs that don't reference relative paths.
///
/// Returns `(Config, IniWarnings)`.  For TOML, `had_trailing_comments` is
/// always `false`.
pub fn parse_from_str(
    content: &str,
    format: ConfigFormat,
    opts: LoadOptions,
) -> Result<(Config, IniWarnings), ParseError> {
    let (mut cfg, warnings) = match format {
        ConfigFormat::Toml => (parse_from_toml_str(content)?, IniWarnings::default()),
        ConfigFormat::Ini => parse_from_ini_str(content)?,
    };
    cfg.normalize(&opts)?;
    cfg.validate()?;
    Ok((cfg, warnings))
}

/// Read a config file from disk and dispatch to [`parse_from_str`] based on
/// the file extension (case-insensitive).
///
/// * `.toml`                  → TOML parser
/// * `.ini`, `.cfg`, `.txt`   → legacy INI parser
/// * anything else / missing  → [`ParseError::UnsupportedFormat`]
///
/// `opts.config_dir` is set to the file's parent directory so relative paths
/// in the config resolve correctly; any value the caller had set is overwritten.
///
/// Returns `(Config, IniWarnings)`.  For TOML files `had_trailing_comments`
/// is always `false`.
pub fn parse_from_file<P: AsRef<Path>>(
    path: P,
    mut opts: LoadOptions,
) -> Result<(Config, IniWarnings), ParseError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)?;
    let format = ConfigFormat::from_path(path)?;
    opts.config_dir = path.parent().map(Path::to_path_buf);
    parse_from_str(&contents, format, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_opts() -> LoadOptions {
        LoadOptions::default()
    }

    #[test]
    fn parse_from_str_toml_minimal() {
        let (cfg, _) =
            parse_from_str("network_quorum = 3", ConfigFormat::Toml, default_opts()).unwrap();
        assert_eq!(cfg.network_quorum, Some(3));
    }

    #[test]
    fn parse_from_str_surfaces_parse_errors() {
        let err =
            parse_from_str("not_a_real_key = 1", ConfigFormat::Toml, default_opts()).unwrap_err();
        assert!(matches!(err, ParseError::Toml(_)), "got {err:?}");
    }
}
