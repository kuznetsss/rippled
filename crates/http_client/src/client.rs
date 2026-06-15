//! Global `reqwest::Client` lifecycle and TLS context management.
//!
//! Mirrors the structure of `runtime.rs`: a single `OnceLock`-protected
//! `RwLock<Option<…>>` slot that can be initialised, replaced, and cleared
//! from any thread.
//!
//! # Why a global client?
//!
//! `reqwest::Client` manages an internal connection pool.  Re-creating it on
//! every request would defeat connection reuse.  A single, lazily-built
//! instance is stored here and cloned cheaply (the clone shares the
//! `Arc`-backed pool).
//!
//! # Thread-safety
//!
//! All public functions acquire the `RwLock` for the minimum duration needed:
//! - `init_tls_context` and `reset_tls_context` take a write guard.
//! - `current` takes a read guard and immediately clones the `Client` before
//!   releasing it.

use crate::error::{Error, Result};
use crate::ffi::{Status, TlsConfig};
use reqwest::Certificate;
use std::{
    fs,
    sync::{OnceLock, RwLock},
};

/// Global slot that holds the live `reqwest::Client`.
///
/// `OnceLock` is used so the `RwLock` wrapper itself is always present; the
/// `Option` inside tracks whether a client has been initialised.
// TODO: maybe we could store client in a better way?
static CLIENT: OnceLock<RwLock<Option<reqwest::Client>>> = OnceLock::new();

/// Return the `RwLock`, initialising the `OnceLock` on first access.
fn slot() -> &'static RwLock<Option<reqwest::Client>> {
    CLIENT.get_or_init(|| RwLock::new(None))
}

// ---------------------------------------------------------------------------
// Public FFI-facing functions (bodies called from ffi.rs shims)
// ---------------------------------------------------------------------------

/// Build and store the global `reqwest::Client` from `config`.
///
/// Safe to call repeatedly — each call atomically replaces the stored client,
/// which causes the previous connection pool to drain naturally once all
/// in-flight requests have completed.
///
/// Returns `Err(Error::CertificateReading(_))` if a certificate file/directory cannot
/// be read from disk, and `Err(Error::TlsConfig(_))` if the `reqwest` builder
/// rejects the configuration.
pub(crate) fn init_tls_context(config: TlsConfig) -> Status {
    build_and_store(config).into()
}

/// Drop the stored `reqwest::Client`.
///
/// A no-op (returns `Ok`) if no client is currently stored, matching the
/// C++ `gHttpClientSslContext.reset()` semantics.
pub(crate) fn reset_tls_context() -> Status {
    let result: Result<()> = (|| {
        *slot().write().map_err(|_| Error::LockPoisoned)? = None;
        Ok(())
    })();
    result.into()
}

/// Clone the stored client, or return `Err(Error::NotInitialized)`.
///
/// `reqwest::Client` is internally `Arc`-backed; the clone is O(1) and
/// shares the connection pool.
pub(crate) fn get() -> Result<reqwest::Client> {
    let guard = slot().read().map_err(|_| Error::LockPoisoned)?;
    guard.as_ref().cloned().ok_or(Error::NotInitialized)
}

// ---------------------------------------------------------------------------
// Internal builder
// ---------------------------------------------------------------------------

fn build_and_store(config: TlsConfig) -> Result<()> {
    let client = build_client(&config)?;
    *slot().write().map_err(|_| Error::LockPoisoned)? = Some(client);
    Ok(())
}

/// Translate `TlsConfig` into a `reqwest::Client`.
///
/// CA mapping (mirrors `HTTPClientSSLContext` semantics):
/// - `verify == false` → skip all certificate and hostname verification
///   entirely (both [`tls_danger_accept_invalid_certs`] and
///   [`tls_danger_accept_invalid_hostnames`] are set).
/// - `verify == true`:
///   - `verify_file` non-empty → use *only* the certs in that PEM bundle
///     (built-in/native roots disabled) via [`tls_certs_only`].
///   - `verify_file` empty → use platform / webpki native roots (default).
///   - `verify_dir` non-empty (independent) → load every parseable PEM file
///     in the directory and merge them into the trust store via
///     [`tls_certs_merge`].
///
/// # reqwest 0.13.4 API notes
///
/// The older `tls_built_in_root_certs(false)` + `add_root_certificate` loop
/// pattern was removed in 0.13.  The replacements are:
/// - [`ClientBuilder::tls_certs_only`] — disables native roots and installs
///   *only* the supplied certs in one call.
/// - [`ClientBuilder::tls_certs_merge`] — adds certs on top of whatever roots
///   are already active (native or those set by `tls_certs_only`).
///
/// [`tls_danger_accept_invalid_certs`]: reqwest::ClientBuilder::tls_danger_accept_invalid_certs
/// [`tls_danger_accept_invalid_hostnames`]: reqwest::ClientBuilder::tls_danger_accept_invalid_hostnames
/// [`tls_certs_only`]: reqwest::ClientBuilder::tls_certs_only
/// [`tls_certs_merge`]: reqwest::ClientBuilder::tls_certs_merge
fn build_client(config: &TlsConfig) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        // The old C++ client used HTTP/1.0 Connection: close and never
        // followed redirects.  Preserve that behaviour.
        .redirect(reqwest::redirect::Policy::none());

    if !config.verify {
        // Disable all TLS verification — both cert chain and hostname.
        // `tls_danger_accept_invalid_certs` / `tls_danger_accept_invalid_hostnames`
        // are the non-deprecated names in reqwest 0.13.4.
        builder = builder
            .tls_danger_accept_invalid_certs(true)
            .tls_danger_accept_invalid_hostnames(true);
    } else {
        // verify_file: replace built-in roots with the supplied bundle.
        // `tls_certs_only` disables native/built-in roots *and* installs the
        // provided certs as the sole trust anchors — the 0.13.4 replacement for
        // `tls_built_in_root_certs(false)` + `add_root_certificate` loops.
        if !config.verify_file.is_empty() {
            let pem = fs::read(&*config.verify_file).map_err(Error::CertificateReading)?;
            let certs = Certificate::from_pem_bundle(&pem).map_err(Error::TlsConfig)?;
            builder = builder.tls_certs_only(certs);
        }
        // verify_dir: add every parseable PEM file on top of whatever roots
        // are already in effect (file or native).
        // `tls_certs_merge` is the 0.13.4 replacement for repeated
        // `add_root_certificate` calls — it accepts any `IntoIterator<Item =
        // Certificate>` so all certs can be passed in a single builder call.
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
