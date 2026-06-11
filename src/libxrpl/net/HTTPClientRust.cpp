#include <xrpl/net/HTTPClientRust.h>

#include <boost/system/errc.hpp>
#include <boost/system/error_code.hpp>

#include <cstddef>
#include <memory>
#include <string>

namespace xrpl::detail {

boost::system::error_code
toErrorCode(::rs::http_client::RequestError code)
{
    namespace errc = boost::system::errc;

    switch (code)
    {
        case ::rs::http_client::RequestError::Ok:
            return {};  // default-constructed = no error
        case ::rs::http_client::RequestError::Timeout:
            return errc::make_error_code(errc::timed_out);
        case ::rs::http_client::RequestError::Connect:
            return errc::make_error_code(errc::connection_refused);
        case ::rs::http_client::RequestError::Dns:
            return errc::make_error_code(errc::host_unreachable);
        case ::rs::http_client::RequestError::Tls:
            return errc::make_error_code(errc::protocol_error);
        case ::rs::http_client::RequestError::BadStatus:
            return errc::make_error_code(errc::bad_address);
        case ::rs::http_client::RequestError::TooLarge:
            return errc::make_error_code(errc::value_too_large);
        case ::rs::http_client::RequestError::Canceled:
            return errc::make_error_code(errc::operation_canceled);
    }
    // Unreachable; silence compiler warnings.
    return errc::make_error_code(errc::operation_canceled);
}

void
startRequest(::rs::http_client::Request request, std::unique_ptr<HttpCompletion> completion)
{
    // Hand the raw pointer across the FFI boundary as an opaque token. We
    // manage its lifetime manually until either Rust calls back (success) or
    // we reclaim it below (enqueue failure).
    auto* rawCompletion = completion.release();
    auto token = reinterpret_cast<std::size_t>(rawCompletion);

    ::rs::http_client::Status status = ::rs::http_client::http_request(std::move(request), token);

    if (status.code != ::rs::http_client::ErrorCode::Ok)
    {
        // Enqueue failed — Rust will never call back.  Reclaim ownership and
        // complete the handler with an error.  All lifecycle failures
        // (NotInitialized, ShutDown, LockPoisoned, …) map to Canceled at the
        // per-request level.
        std::unique_ptr<HttpCompletion> reclaimed(rawCompletion);
        ::rs::http_client::RequestResult result{
            .code = ::rs::http_client::RequestError::Canceled,
            .message = ::rust::String(static_cast<std::string>(status.message)),
            .response = ::rs::http_client::Response{.status = 0, .headers = {}, .body = {}},
        };
        reclaimed->complete(std::move(result));
        // `reclaimed` frees the State on scope exit.
    }
    // On success Rust owns `raw`; nothing to free here.
}

}  // namespace xrpl::detail

// One-line shim in the rs::http_client namespace as required by the cxx bridge.
// Must be defined in the rs::http_client namespace so the linker finds the symbol
// declared in HttpClientCallback.h / the generated ffi.h.
namespace rs::http_client {

void
resume_http_request(::std::size_t completion, ::rs::http_client::RequestResult result)
{
    // Reclaim unique_ptr from the raw pointer; `complete` posts the handler;
    // the unique_ptr destructs the State on scope exit.
    auto c = std::unique_ptr<::xrpl::detail::HttpCompletion>(
        reinterpret_cast<::xrpl::detail::HttpCompletion*>(completion));
    c->complete(std::move(result));
}

}  // namespace rs::http_client
