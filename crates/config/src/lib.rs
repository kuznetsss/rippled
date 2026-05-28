pub mod error;
pub mod ffi;
pub mod ini;
pub mod schema;

use std::fs;
use std::path::Path;

pub use crate::error::ParseError;
use crate::ini::parse_ini;
use crate::schema::{Config, ValidatorData};

/// Recognised config file formats, determined by file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigExtension {
    Toml,
    Ini,
}

impl ConfigExtension {
    /// Determine format from `path`'s extension (case-insensitive).
    ///
    /// * `.toml`            → `Toml`
    /// * `.ini`, `.cfg`, `.txt` → `Ini`
    /// * anything else / missing → `Err(ParseError::UnsupportedFormat)`
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
/// If the parsed config contains a `validators_file` path, that file is also
/// loaded and parsed using [`ConfigExtension::from_path`] — unknown extensions
/// error for both the main config and the validators file. Its validator data
/// is merged into the returned config via [`Config::merge_validators`].
/// Relative paths in `validators_file` are resolved against the parent
/// directory of the main config file.
///
/// Returns `(Config, IniWarnings)`.  For TOML files `had_trailing_comments`
/// is always `false`.
pub fn parse_from_file<P: AsRef<Path>>(path: P) -> Result<(Config, IniWarnings), ParseError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)?;

    let (mut cfg, warnings) = match ConfigExtension::from_path(path)? {
        ConfigExtension::Toml => {
            let cfg = parse_from_toml_str(&contents)?;
            (cfg, IniWarnings::default())
        }
        ConfigExtension::Ini => parse_from_ini_str(&contents)?,
    };

    // If the main config specifies a validators_file, load and merge it.
    if let Some(validators_path) = cfg.validators_file.take() {
        // Resolve relative paths against the main config file's parent directory.
        let validators_path = if validators_path.is_absolute() {
            validators_path
        } else {
            path.parent()
                .unwrap_or(Path::new("."))
                .join(&validators_path)
        };

        let v_contents = fs::read_to_string(&validators_path)?;

        let (validator_data, strict) = match ConfigExtension::from_path(&validators_path)? {
            ConfigExtension::Toml => (toml::from_str::<ValidatorData>(&v_contents)?, true),
            ConfigExtension::Ini => {
                let bc = parse_ini(&v_contents);
                (ini::validators_from_basic_config(&bc)?, false)
            }
        };

        cfg.validators_file = Some(validators_path);
        cfg.merge_validators(validator_data, strict)?;
    }

    // Fix 1: Consolidate [validator_keys] into [validators], matching C++:
    //   section(SECTION_VALIDATORS).append(section(SECTION_VALIDATOR_KEYS).lines());
    // This is done unconditionally (whether or not a validators file was loaded).
    let extra = cfg.validator_keys.clone();
    cfg.validators.extend(extra);

    // Fix 3: Validate validator_list_threshold if set.
    if let Some(threshold) = cfg.validator_list_threshold {
        if threshold == 0 {
            return Err(ParseError::Ini(
                "validator_list_threshold must be greater than 0".into(),
            ));
        }
        if threshold as usize > cfg.validator_list_keys.len() {
            return Err(ParseError::Ini(format!(
                "validator_list_threshold ({threshold}) exceeds the number of validator_list_keys ({})",
                cfg.validator_list_keys.len()
            )));
        }
    }

    Ok((cfg, warnings))
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

    // -----------------------------------------------------------------------
    // Fix 1: validator_keys entries are consolidated into validators
    // -----------------------------------------------------------------------

    #[test]
    fn fix1_validator_keys_consolidated_into_validators_no_validators_file() {
        let dir =
            std::env::temp_dir().join(format!("config-fix1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("xrpld.cfg");
        // Main config has entries in both [validators] and [validator_keys].
        // [validator_list_keys] is provided so threshold validation can pass.
        std::fs::write(
            &cfg_path,
            "[validators]\nnVAL1\n\n[validator_keys]\nnKEY1\nnKEY2\n\
             \n[validator_list_keys]\nhexkey1\nhexkey2\nhexkey3\n",
        )
        .unwrap();

        let (cfg, _) = parse_from_file(&cfg_path).unwrap();
        // After consolidation, validators must include the original entry
        // plus the two from validator_keys.
        assert!(
            cfg.validators.contains(&"nVAL1".to_string()),
            "original validator missing"
        );
        assert!(
            cfg.validators.contains(&"nKEY1".to_string()),
            "nKEY1 from validator_keys missing"
        );
        assert!(
            cfg.validators.contains(&"nKEY2".to_string()),
            "nKEY2 from validator_keys missing"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // Fix 2: validators file threshold wins over main config threshold
    // -----------------------------------------------------------------------

    #[test]
    fn fix2_validators_file_threshold_wins_over_main_config() {
        let dir =
            std::env::temp_dir().join(format!("config-fix2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Validators file: threshold=2, two list keys.
        let vfile_path = dir.join("validators.txt");
        std::fs::write(
            &vfile_path,
            "[validator_list_keys]\nhexkey1\nhexkey2\n\
             \n[validator_list_threshold]\n2\n",
        )
        .unwrap();

        // Main config: also sets threshold=5 (should be overwritten by file).
        let cfg_path = dir.join("xrpld.cfg");
        std::fs::write(
            &cfg_path,
            &format!(
                "[validators_file]\n{}\n\n[validator_list_threshold]\n5\n",
                vfile_path.display()
            ),
        )
        .unwrap();

        let (cfg, _) = parse_from_file(&cfg_path).unwrap();
        // The validators file threshold (2) must win.
        assert_eq!(
            cfg.validator_list_threshold,
            Some(2),
            "validators file threshold should overwrite main config threshold"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // -----------------------------------------------------------------------
    // Fix 3: validator_list_threshold validation
    // -----------------------------------------------------------------------

    #[test]
    fn fix3_threshold_zero_is_error() {
        let dir =
            std::env::temp_dir().join(format!("config-fix3a-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("xrpld.cfg");
        std::fs::write(
            &cfg_path,
            "[validator_list_keys]\nhexkey1\n\n[validator_list_threshold]\n0\n",
        )
        .unwrap();

        let err = parse_from_file(&cfg_path).unwrap_err();
        assert!(
            matches!(&err, ParseError::Ini(msg) if msg.contains("greater than 0")),
            "expected threshold>0 error, got: {err:?}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fix3_threshold_exceeds_keys_count_is_error() {
        let dir =
            std::env::temp_dir().join(format!("config-fix3b-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("xrpld.cfg");
        // 1 key but threshold=5.
        std::fs::write(
            &cfg_path,
            "[validator_list_keys]\nhexkey1\n\n[validator_list_threshold]\n5\n",
        )
        .unwrap();

        let err = parse_from_file(&cfg_path).unwrap_err();
        assert!(
            matches!(&err, ParseError::Ini(msg) if msg.contains("exceeds")),
            "expected threshold>keys error, got: {err:?}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fix3_valid_threshold_succeeds() {
        let dir =
            std::env::temp_dir().join(format!("config-fix3c-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("xrpld.cfg");
        // 3 keys, threshold=2: valid.
        std::fs::write(
            &cfg_path,
            "[validator_list_keys]\nhexkey1\nhexkey2\nhexkey3\n\
             \n[validator_list_threshold]\n2\n",
        )
        .unwrap();

        let (cfg, _) = parse_from_file(&cfg_path).unwrap();
        assert_eq!(cfg.validator_list_threshold, Some(2));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
