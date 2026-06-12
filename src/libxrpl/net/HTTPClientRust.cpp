#include <xrpl/net/HTTPClientRust.h>

#include <boost/system/errc.hpp>
#include <boost/system/error_code.hpp>

#include <memory>

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
startRequest(::rs::http_client::Request request, std::unique_ptr<HTTPCompletion> completion)
{
    // Ownership of `completion` moves into Rust.  On enqueue failure Rust
    // drops the UniquePtr<HTTPCompletion> before the task runs, and the
    // CompletionGuard's Drop impl calls complete() with Canceled — so there
    // is nothing to reclaim here.
    ::rs::http_client::http_request(std::move(request), std::move(completion));
}

}  // namespace xrpl::detail
