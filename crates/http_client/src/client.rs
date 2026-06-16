use crate::error::{Error, Result};
use crate::ffi::{Status, TlsConfig};
use reqwest::Certificate;
use std::{
    fs,
    sync::{OnceLock, RwLock},
};

// TODO: maybe we could store client in a better way?
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

pub(crate) fn get() -> Result<reqwest::Client> {
    let guard = slot().read().map_err(|_| Error::LockPoisoned)?;
    guard.as_ref().cloned().ok_or(Error::NotInitialized)
}

fn build_and_store(config: TlsConfig) -> Result<()> {
    let client = build_client(&config)?;
    *slot().write().map_err(|_| Error::LockPoisoned)? = Some(client);
    Ok(())
}

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
