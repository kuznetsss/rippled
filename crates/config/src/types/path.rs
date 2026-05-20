use std::path::{Path, PathBuf};
use serde::{Deserialize, Deserializer, Serialize};

/// A path that `Config::bootstrap()` will resolve relative to the config
/// directory. Stored as-parsed; only absolutized during bootstrap.
///
/// This is distinct from a plain `PathBuf` so that the resolution policy is
/// visible in the schema: callers that receive a `RelPath` know it may still be
/// relative until bootstrap has run.
///
/// Deserializes from a string (not from a `{ "0": "..." }` tuple struct form).
/// This makes TOML schema deserialization work correctly for types like `PerfConfig`
/// that embed `Option<RelPath>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelPath(pub PathBuf);

impl<'de> Deserialize<'de> for RelPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(RelPath(PathBuf::from(s)))
    }
}

impl RelPath {
    pub fn new(p: PathBuf) -> Self {
        RelPath(p)
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for RelPath {
    fn from(p: PathBuf) -> Self {
        RelPath(p)
    }
}

impl std::fmt::Display for RelPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// Resolve `p` against `base`.
/// - If `p` is already absolute, return it unchanged.
/// - Otherwise return `base.join(p)`.
pub fn resolve_against(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_owned()
    } else {
        base.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_passes_through() {
        let base = Path::new("/etc/xrpld");
        let abs = Path::new("/var/lib/xrpld/db");
        assert_eq!(resolve_against(base, abs), PathBuf::from("/var/lib/xrpld/db"));
    }

    #[test]
    fn relative_joins() {
        let base = Path::new("/etc/xrpld");
        let rel = Path::new("db");
        assert_eq!(resolve_against(base, rel), PathBuf::from("/etc/xrpld/db"));
    }

    // ---- additional coverage ----

    #[test]
    fn relpath_carries_pathbuf() {
        let pb = PathBuf::from("some/relative/path");
        let rp = RelPath::new(pb.clone());
        assert_eq!(rp.as_path(), pb.as_path());
    }

    #[test]
    fn relpath_from_pathbuf() {
        let pb = PathBuf::from("another/path");
        let rp: RelPath = pb.clone().into();
        assert_eq!(rp.0, pb);
    }

    #[test]
    fn relpath_display() {
        let rp = RelPath::new(PathBuf::from("config/data"));
        assert_eq!(rp.to_string(), "config/data");
    }

    #[test]
    fn relpath_equality() {
        let a = RelPath::new(PathBuf::from("foo/bar"));
        let b = RelPath::new(PathBuf::from("foo/bar"));
        assert_eq!(a, b);
    }

    #[test]
    fn relpath_inequality() {
        let a = RelPath::new(PathBuf::from("foo/bar"));
        let b = RelPath::new(PathBuf::from("foo/baz"));
        assert_ne!(a, b);
    }

    #[test]
    fn empty_relative_joins_to_base() {
        let base = Path::new("/etc/xrpld");
        let rel = Path::new("");
        // An empty path joined to base should equal base (PathBuf::join behavior)
        let result = resolve_against(base, rel);
        assert_eq!(result, PathBuf::from("/etc/xrpld/"));
    }

    #[test]
    fn dot_relative_joins_to_base() {
        let base = Path::new("/etc/xrpld");
        let rel = Path::new(".");
        let result = resolve_against(base, rel);
        assert_eq!(result, PathBuf::from("/etc/xrpld/."));
    }

    #[test]
    fn dotdot_relative_joins_to_base() {
        let base = Path::new("/etc/xrpld");
        let rel = Path::new("..");
        let result = resolve_against(base, rel);
        // PathBuf::join does not canonicalize, so we get the raw joined path
        assert_eq!(result, PathBuf::from("/etc/xrpld/.."));
    }

    #[test]
    fn nested_relative_joins() {
        let base = Path::new("/var/lib");
        let rel = Path::new("xrpld/db");
        assert_eq!(resolve_against(base, rel), PathBuf::from("/var/lib/xrpld/db"));
    }

    #[test]
    fn absolute_path_ignores_base() {
        let base = Path::new("/tmp/base");
        let abs = Path::new("/absolute/path");
        assert_eq!(resolve_against(base, abs), PathBuf::from("/absolute/path"));
    }
}
