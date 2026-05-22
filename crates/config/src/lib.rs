pub mod error;
pub mod ffi;
pub mod schema;

use std::fs;
use std::path::Path;

pub use crate::error::ParseError;
use crate::schema::Config;

/// Parse a `Config` from an in-memory TOML document.
pub fn parse_from_toml_str(s: &str) -> Result<Config, ParseError> {
    Ok(toml::from_str(s)?)
}

/// Parse a `Config` from an in-memory legacy INI document.
pub fn parse_from_ini_str(_s: &str) -> Result<Config, ParseError> {
    todo!("legacy INI parser not yet implemented")
}

/// Read a config file from disk and dispatch to the appropriate parser based
/// on the file extension (case-insensitive).
///
/// * `.toml`                  → TOML parser
/// * `.ini`, `.cfg`, `.txt`   → legacy INI parser
/// * anything else / missing  → [`ParseError::UnsupportedFormat`]
pub fn parse_from_file<P: AsRef<Path>>(path: P) -> Result<Config, ParseError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "toml" => parse_from_toml_str(&contents),
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

        let cfg = parse_from_file(&toml_path).unwrap();
        assert_eq!(cfg.network_quorum, Some(7));

        std::fs::remove_file(&toml_path).unwrap();
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
    #[should_panic(expected = "not yet implemented")]
    fn parse_from_ini_str_is_todo() {
        let _ = parse_from_ini_str("");
    }
}
