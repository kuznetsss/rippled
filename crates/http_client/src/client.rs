//! Global `reqwest::Client` and TLS context management.
//!
//! `CLIENT` holds an `Option<reqwest::Client>` behind an `RwLock` so that
//! `reset_tls_context` can clear it between successive `init_tls_context`
//! calls without re-seating the `OnceLock` slot.

use crate::error::{Error, Result};
use crate::ffi::{Status, TlsConfig};
use reqwest::Certificate;
use std::{
    fs,
    sync::{OnceLock, RwLock},
};

static CLIENT: OnceLock<RwLock<Option<reqwest::Client>>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<reqwest::Client>> {
    CLIENT.get_or_init(|| RwLock::new(None))
}

pub(crate) fn init_tls_context(config: TlsConfig) -> Status {
    build_and_store(config).into()
}

pub(crate) fn reset_tls_context() -> Status {
    let result: Result<()> = (|| {
        *slot().write().map_err(|_| Error::LockPoisoned)? = None;
        Ok(())
    })();
    result.into()
}

/// Return a clone of the shared client, or `NotInitialized` if TLS context has not been set.
pub(crate) fn get() -> Result<reqwest::Client> {
    let guard = slot().read().map_err(|_| Error::LockPoisoned)?;
    guard.as_ref().cloned().ok_or(Error::NotInitialized)
}

fn build_and_store(config: TlsConfig) -> Result<()> {
    let client = build_client(&config)?;
    *slot().write().map_err(|_| Error::LockPoisoned)? = Some(client);
    Ok(())
}

/// Construct a `reqwest::Client` from `config`.
///
/// Redirects are disabled to preserve the legacy C++ client behaviour.
/// When `verify` is `true`, `verify_file` (if set) replaces the default CA
/// roots entirely; `verify_dir` (if set) adds additional certs on top.
fn build_client(config: &TlsConfig) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        // Preserve legacy C++ client behaviour: no redirect following.
        .redirect(reqwest::redirect::Policy::none());

    if !config.verify {
        builder = builder
            .tls_danger_accept_invalid_certs(true)
            .tls_danger_accept_invalid_hostnames(true);
    } else {
        if !config.verify_file.is_empty() {
            // tls_certs_only disables native roots and installs only the
            // supplied bundle as trust anchors (reqwest 0.13 API).
            let pem = fs::read(&*config.verify_file).map_err(Error::CertificateReading)?;
            let certs = Certificate::from_pem_bundle(&pem).map_err(Error::TlsConfig)?;
            builder = builder.tls_certs_only(certs);
        }
        if !config.verify_dir.is_empty() {
            let entries = fs::read_dir(&*config.verify_dir).map_err(Error::CertificateReading)?;
            let dir_certs: Vec<Certificate> = entries
                .flatten()
                .filter(|e| e.path().is_file())
                .flat_map(|e| {
                    let Ok(pem) = fs::read(e.path()) else {
                        return vec![];
                    };
                    // Try multi-cert bundle first; fall back to single-cert parse.
                    Certificate::from_pem_bundle(&pem)
                        .or_else(|_| Certificate::from_pem(&pem).map(|c| vec![c]))
                        .unwrap_or_default()
                })
                .collect();
            if !dir_certs.is_empty() {
                builder = builder.tls_certs_merge(dir_certs);
            }
        }
    }

    builder.build().map_err(Error::TlsConfig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::ErrorCode;

    // ── build_client tests (global-free, plain #[test]) ───────────────────────

    /// `verify: false` disables certificate checking; the builder should succeed.
    #[test]
    fn build_client_verify_false() {
        let cfg = TlsConfig {
            verify: false,
            verify_file: String::new(),
            verify_dir: String::new(),
        };
        assert!(build_client(&cfg).is_ok());
    }

    /// `verify: true` with no custom paths uses the default system roots.
    #[test]
    fn build_client_verify_true_defaults() {
        let cfg = TlsConfig {
            verify: true,
            verify_file: String::new(),
            verify_dir: String::new(),
        };
        assert!(build_client(&cfg).is_ok());
    }

    /// A `verify_file` that does not exist must produce `CertificateReading`.
    #[test]
    fn build_client_nonexistent_verify_file() {
        let cfg = TlsConfig {
            verify: true,
            verify_file: "/this/path/does/not/exist.pem".to_string(),
            verify_dir: String::new(),
        };
        let err = build_client(&cfg).unwrap_err();
        assert!(matches!(err, Error::CertificateReading(_)));
    }

    /// A file with a valid PEM header but invalid base64 body must produce
    /// `TlsConfig` — reqwest returns an error for malformed base64 inside a PEM
    /// section (pure garbage with no PEM markers is silently skipped instead).
    #[test]
    fn build_client_garbage_verify_file() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        // Must have a PEM header so the parser enters the section and then
        // fails on the non-base64 body — pure-garbage files yield Ok(vec![]).
        std::io::Write::write_all(
            &mut tmp,
            b"-----BEGIN CERTIFICATE-----\nnot_valid_base64_garbage!!!\n-----END CERTIFICATE-----\n",
        )
        .unwrap();
        let cfg = TlsConfig {
            verify: true,
            verify_file: tmp.path().to_str().unwrap().to_string(),
            verify_dir: String::new(),
        };
        let err = build_client(&cfg).unwrap_err();
        assert!(matches!(err, Error::TlsConfig(_)));
    }

    /// A `verify_dir` containing only junk files (no valid certs) is silently
    /// skipped — the builder should still succeed (no certs merged).
    #[test]
    fn build_client_junk_verify_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("junk.pem"), b"not a cert").unwrap();
        let cfg = TlsConfig {
            verify: true,
            verify_file: String::new(),
            verify_dir: dir.path().to_str().unwrap().to_string(),
        };
        // Junk files are silently skipped; no error expected.
        assert!(build_client(&cfg).is_ok());
    }

    // NOTE: A test for a valid verify_file containing a real PEM certificate is
    // intentionally omitted — generating a proper cert fixture is out of scope.

    // ── Global lifecycle test (serial, must be isolated) ─────────────────────

    /// Verifies the full init → get → reset cycle for the CLIENT global.
    /// Runs with `#[serial]` so no other test observes intermediate state.
    #[serial_test::serial]
    #[test]
    fn tls_context_lifecycle() {
        let _ = reset_tls_context(); // drive to known-clean state
        assert!(matches!(get(), Err(Error::NotInitialized)));
        let cfg = TlsConfig {
            verify: false,
            verify_file: String::new(),
            verify_dir: String::new(),
        };
        assert!(matches!(init_tls_context(cfg).code, ErrorCode::Ok));
        assert!(get().is_ok());
        assert!(matches!(reset_tls_context().code, ErrorCode::Ok));
        assert!(matches!(get(), Err(Error::NotInitialized)));
    }
}
