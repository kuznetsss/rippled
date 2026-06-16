use crate::client::{init_tls_context, reset_tls_context};
use crate::runtime::Runtime;
use std::time::Duration;

#[cxx::bridge(namespace = "rs::http_client")]
mod bridge {
    #[derive(Debug)]
    enum ErrorCode {
        Ok,
        RuntimeBuild,
        AlreadyInitialized,
        NotInitialized,
        ShutDown,
        LockPoisoned,
        CertificateReading,
        TlsConfig,
    }

    struct Status {
        code: ErrorCode,
        message: String,
    }

    #[cxx_name = "HTTPMethod"]
    enum HttpMethod {
        Get,
        Post,
    }

    #[cxx_name = "HTTPHeader"]
    struct HttpHeader {
        name: String,
        value: String,
    }

    struct Request {
        method: HttpMethod,
        url: String,
        headers: Vec<HttpHeader>,
        timeout_ms: u64,
        max_response_bytes: usize,
    }

    struct Response {
        status: u16,
        headers: Vec<HttpHeader>,
        body: Vec<u8>,
    }

    #[derive(Debug)]
    enum RequestError {
        Ok,
        Timeout,
        // reqwest does not expose a typed way to tell connect/DNS/TLS apart;
        // specific cause is preserved in RequestResult::message.
        Failed,
        TooLarge,
        Canceled,
        NotInitialized,
    }

    struct TlsConfig {
        verify: bool,
        /// Path to a PEM bundle that replaces the default CA roots.
        verify_file: String,
        /// Path to a directory of PEM certificates to add on top of active roots.
        verify_dir: String,
    }

    struct RequestResult {
        code: RequestError,
        message: String,
        response: Response,
    }

    extern "Rust" {
        fn init_tokio_runtime(threads_num: usize) -> Status;
        fn shutdown_tokio_runtime(timeout_ms: u64) -> Status;
        fn init_tls_context(config: TlsConfig) -> Status;
        fn reset_tls_context() -> Status;
        fn http_request(request: Request, body: &[u8], completion: UniquePtr<HttpCompletion>);
    }

    unsafe extern "C++" {
        include!("xrpl/net/detail/HTTPCompletion.h");

        #[namespace = "xrpl::detail"]
        #[cxx_name = "HTTPCompletion"]
        type HttpCompletion;

        fn complete(self: Pin<&mut HttpCompletion>, result: RequestResult);
    }
}

pub(crate) use bridge::{
    ErrorCode, HttpCompletion, HttpHeader, HttpMethod, Request, RequestError, RequestResult,
    Response, Status, TlsConfig,
};

// SAFETY: `HttpCompletion` is accessed only via `complete()`, which posts work
// onto a thread-safe Asio executor.  It is never aliased and is consumed
// exactly once, so moving it across thread boundaries is safe.
unsafe impl Send for HttpCompletion {}

use crate::request::http_request;

fn init_tokio_runtime(threads_num: usize) -> Status {
    Runtime::init(threads_num).into()
}

fn shutdown_tokio_runtime(timeout_ms: u64) -> Status {
    Runtime::shutdown(Duration::from_millis(timeout_ms)).into()
}
