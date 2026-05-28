use std::path::PathBuf;

/// Inputs to [`Config::normalize`](crate::schema::Config::normalize).
///
/// Carries:
/// - CLI overrides (`standalone`, `quorum_override`) — only two CLI flags are
///   relevant to the Rust side.
/// - `config_dir` — the directory that relative paths in the config (e.g.
///   `database_path`, `debug_logfile`, `validators_file`) resolve against.
///   Set automatically by [`parse_from_file`](crate::parse_from_file); callers
///   of [`parse_from_str`](crate::parse_from_str) leave it `None` unless they
///   want relative-path resolution.
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// Set when `--standalone` / `-a` is given on the CLI.
    pub standalone: bool,
    /// Set when `--quorum <n>` is given on the CLI.  Must be non-zero.
    pub quorum_override: Option<u32>,
    /// Directory that relative paths in the config resolve against.
    pub config_dir: Option<PathBuf>,
}

impl LoadOptions {
    /// Set the standalone flag.
    pub fn set_standalone(&mut self, value: bool) {
        self.standalone = value;
    }

    /// Set the quorum override.  A value of `0` is treated as "not set"
    /// on the C++ side, but `Option<u32>` here captures the intent directly.
    pub fn set_quorum_override(&mut self, value: u32) {
        self.quorum_override = Some(value);
    }

    /// Set the directory used to resolve relative paths in the config.
    /// Accepts `&str` so it can be called from C++ via cxx-bridge.
    pub fn set_config_dir(&mut self, path: &str) {
        self.config_dir = Some(PathBuf::from(path));
    }
}
