//! Primitive `Optional*` wrapper types.
//!
//! `Optional<T>` wraps a native `Option<T>` and exposes a uniform method API
//! to C++ via the cxx bridge. `value()` returns `Result<T>` so misuse
//! (calling it on an empty wrapper) throws on the C++ side — same semantics
//! as `std::optional::value()` throwing `bad_optional_access`.
//!
//! The C++-facing names (`OptionalU32`, `OptionalString`, …) come from the
//! aliases below; cxx generates one C++ class per alias because each is a
//! distinct generic instantiation.

pub struct Optional<T>(pub(crate) Option<T>);

impl<T> From<Option<T>> for Optional<T> {
    fn from(v: Option<T>) -> Self {
        Self(v)
    }
}

impl<T: Clone> Optional<T> {
    pub fn has_value(&self) -> bool {
        self.0.is_some()
    }

    pub fn value(&self) -> Result<T, String> {
        self.0
            .clone()
            .ok_or_else(|| "Optional has no value".to_string())
    }
}

pub type OptionalBool = Optional<bool>;
pub type OptionalU8 = Optional<u8>;
pub type OptionalU16 = Optional<u16>;
pub type OptionalU32 = Optional<u32>;
pub type OptionalU64 = Optional<u64>;
pub type OptionalI32 = Optional<i32>;
pub type OptionalString = Optional<String>;

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_outcome(s: &str) -> Box<crate::schema::Config> {
        let mut outcome = crate::ffi::parse_from_toml_str(s);
        assert!(
            outcome.has_value(),
            "parse failed: {}",
            outcome.error().unwrap_or_default()
        );
        outcome.value().expect("has_value=true")
    }

    #[test]
    fn scalar_getter_present() {
        let cfg = ok_outcome("network_quorum = 3");
        let q = cfg.network_quorum();
        assert!(q.has_value());
        assert_eq!(q.value().unwrap(), 3);
    }

    #[test]
    fn scalar_getter_absent_throws_on_value() {
        let cfg = ok_outcome("");
        let q = cfg.network_quorum();
        assert!(!q.has_value());
        assert!(q.value().is_err());
    }

    #[test]
    fn string_getter_handles_pathbuf() {
        let cfg = ok_outcome(r#"debug_logfile = "/var/log/xrpld.log""#);
        let p = cfg.debug_logfile();
        assert!(p.has_value());
        assert_eq!(p.value().unwrap(), "/var/log/xrpld.log");
    }
}
