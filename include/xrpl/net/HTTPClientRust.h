#pragma once

// The detail headers below are implementation fragments of this public header,
// not standalone headers.  This token grants them access; they #error without
// it so they cannot be included directly.  Undefined at the bottom of the file.
#define XRPL_NET_HTTPCLIENTRUST_INTERNAL
#include <xrpl/net/detail/HTTPCompletionImpl.h>

#include <boost/asio/any_io_executor.hpp>
#include <boost/asio/async_result.hpp>
#include <boost/system/error_code.hpp>

#include <rs_http_client_cxxbridge/ffi.h>

#include <memory>
#include <type_traits>
#include <utility>
#include <vector>

namespace xrpl {

namespace detail {

template <class Handler>
struct FailoverState;

}  // namespace detail

/** Asio-compatible async HTTP client backed by a Tokio runtime in Rust.
 *
 *  Provides a single-URL @c asyncRequest composable with any Asio completion
 *  token (lambda, @c use_awaitable, @c use_future, …), and a multi-site
 *  failover helper @c asyncRequestAny that tries each URL in order until one
 *  succeeds or the list is exhausted.
 */
struct HTTPClientRust
{
    /** Issue a single HTTP request.
     *
     *  Completion signature: @c void(boost::system::error_code,
     *                                rs::http_client::Response).
     *
     *  The handler is always dispatched onto the executor associated with
     *  @p token (falling back to @p executor when the token carries none).
     *  The handler is never invoked inline from the calling thread.
     *
     *  Per-operation cancellation is not supported; the timeout is set in
     *  @c request.timeout_ms and enforced by the Rust side.
     *
     *  @param executor  Fallback executor when the token has no associated one.
     *  @param request   Request parameters including URL, method, and timeout.
     *  @param token     Asio completion token.
     */
    template <class CompletionToken>
    static auto
    asyncRequest(
        boost::asio::any_io_executor executor,
        ::rs::http_client::Request request,
        CompletionToken&& token)
    {
        auto initiation = [](auto handler,
                             boost::asio::any_io_executor executor,
                             ::rs::http_client::Request request) {
            using HandlerType = std::decay_t<decltype(handler)>;
            auto completion = std::make_unique<detail::HTTPCompletionImpl<HandlerType>>(
                std::move(handler), std::move(executor));
            detail::startRequest(std::move(request), std::move(completion));
        };
        return boost::asio::async_initiate<
            CompletionToken,
            void(boost::system::error_code, ::rs::http_client::Response)>(
            std::move(initiation), token, std::move(executor), std::move(request));
    }

    /** Try each URL in @p requests in order; complete with the first success.
     *
     *  If all sites fail, completes with the last site's error.  Built on
     *  @c asyncRequest; shares its completion signature.
     *
     *  @param executor  Fallback executor.
     *  @param requests  Ordered list of request parameters to attempt.
     *  @param token     Asio completion token.
     *
     *  @note TODO: per-site timeout overrides, early cancellation, and
     *        structured per-site error logging are not yet implemented.
     */
    template <class CompletionToken>
    static auto
    asyncRequestAny(
        boost::asio::any_io_executor executor,
        std::vector<::rs::http_client::Request> requests,
        CompletionToken&& token)
    {
        auto initiation = [](auto handler,
                             boost::asio::any_io_executor executor,
                             std::vector<::rs::http_client::Request> requests) {
            using HandlerType = std::decay_t<decltype(handler)>;
            // Shared ownership lets the per-site completion lambdas be copyable.
            auto state = std::make_shared<detail::FailoverState<HandlerType>>(
                std::move(executor), std::move(requests), std::move(handler));
            state->next(state);
        };
        return boost::asio::async_initiate<
            CompletionToken,
            void(boost::system::error_code, ::rs::http_client::Response)>(
            std::move(initiation), token, std::move(executor), std::move(requests));
    }
};

}  // namespace xrpl

#include <xrpl/net/detail/FailoverState.ipp>

#undef XRPL_NET_HTTPCLIENTRUST_INTERNAL
