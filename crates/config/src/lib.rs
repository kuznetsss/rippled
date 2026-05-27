pub mod error;
pub mod ffi;
pub mod ini;
pub mod schema;

use std::fs;
use std::path::Path;

pub use crate::error::ParseError;
use crate::ini::parse_ini;
use crate::schema::Config;

/// Warnings emitted when parsing a legacy INI file.
#[derive(Debug, Default, Clone)]
pub struct IniWarnings {
    /// `true` when any section contained trailing `# comments` (after
    /// comment-stripping), matching the behaviour of `BasicConfig::hadTrailingComments()`.
    pub had_trailing_comments: bool,
}

/// Parse a `Config` from an in-memory TOML document.
pub fn parse_from_toml_str(s: &str) -> Result<Config, ParseError> {
    Ok(toml::from_str(s)?)
}

/// Parse a `Config` from an in-memory legacy INI document.
///
/// Returns `(Config, IniWarnings)` on success.
pub fn parse_from_ini_str(s: &str) -> Result<(Config, IniWarnings), ParseError> {
    let bc = parse_ini(s);
    let had_trailing_comments = bc.values().any(|sec| sec.had_trailing_comments);
    let config: Config = ini::from_basic_config(&bc)?;
    Ok((
        config,
        IniWarnings {
            had_trailing_comments,
        },
    ))
}

/// Read a config file from disk and dispatch to the appropriate parser based
/// on the file extension (case-insensitive).
///
/// * `.toml`                  → TOML parser
/// * `.ini`, `.cfg`, `.txt`   → legacy INI parser
/// * anything else / missing  → [`ParseError::UnsupportedFormat`]
///
/// Returns `(Config, IniWarnings)`.  For TOML files `had_trailing_comments`
/// is always `false`.
pub fn parse_from_file<P: AsRef<Path>>(path: P) -> Result<(Config, IniWarnings), ParseError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "toml" => {
            let cfg = parse_from_toml_str(&contents)?;
            Ok((cfg, IniWarnings::default()))
        }
        "ini" | "cfg" | "txt" => parse_from_ini_str(&contents),
        _ => Err(ParseError::UnsupportedFormat(ext)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_from_toml_str_minimal() {
        let cfg = parse_from_toml_str("network_quorum = 3").unwrap();
        assert_eq!(cfg.network_quorum, Some(3));
    }

    #[test]
    fn parse_from_toml_str_surfaces_parse_errors() {
        let err = parse_from_toml_str("not_a_real_key = 1").unwrap_err();
        assert!(matches!(err, ParseError::Toml(_)), "got {err:?}");
    }

    #[test]
    fn parse_from_file_dispatches_by_extension() {
        let dir = std::env::temp_dir().join(format!("config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let toml_path = dir.join("example.toml");
        std::fs::write(&toml_path, "network_quorum = 7\n").unwrap();

        let (cfg, warnings) = parse_from_file(&toml_path).unwrap();
        assert_eq!(cfg.network_quorum, Some(7));
        assert!(!warnings.had_trailing_comments);

        std::fs::remove_file(&toml_path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn parse_from_file_ini_extension() {
        let dir = std::env::temp_dir().join(format!("config-test-ini-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ini_path = dir.join("example.cfg");
        std::fs::write(&ini_path, "[network_quorum]\n3\n").unwrap();

        let (cfg, _) = parse_from_file(&ini_path).unwrap();
        assert_eq!(cfg.network_quorum, Some(3));

        std::fs::remove_file(&ini_path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn parse_from_file_io_error_is_typed() {
        let err = parse_from_file("/nonexistent/path/to/xrpld.toml").unwrap_err();
        assert!(matches!(err, ParseError::Io(_)), "got {err:?}");
    }

    #[test]
    fn parse_from_file_unsupported_extension_errors() {
        let dir = std::env::temp_dir().join(format!("config-test-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("xrpld.yaml");
        std::fs::write(&path, "").unwrap();

        let err = parse_from_file(&path).unwrap_err();
        assert!(
            matches!(err, ParseError::UnsupportedFormat(ref ext) if ext == "yaml"),
            "got {err:?}",
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn parse_from_ini_str_minimal() {
        let (cfg, warnings) = parse_from_ini_str("[network_quorum]\n3").unwrap();
        assert_eq!(cfg.network_quorum, Some(3));
        assert!(!warnings.had_trailing_comments);
    }

    #[test]
    fn parse_from_ini_str_returns_ini_warnings() {
        // A trailing comment should surface in IniWarnings.
        let (_, warnings) = parse_from_ini_str("[network_quorum]\n3 # trailing").unwrap();
        assert!(warnings.had_trailing_comments);
    }
}
