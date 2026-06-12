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

/// Asio-compatible async HTTP client backed by a Tokio runtime in Rust.
///
/// Exposes a single-URL `asyncRequest` that composes with any Asio
/// completion token (lambda, `use_awaitable`, `use_future`, …), and a
/// multi-site failover helper that chains `asyncRequest` calls until one
/// succeeds or the list is exhausted.
struct HTTPClientRust
{
    /// Issue a single HTTP request and complete with
    /// `void(boost::system::error_code, rs::http_client::Response)`.
    ///
    /// The handler is always invoked on the executor associated with the
    /// completion token (falling back to @p ex when the token has none).
    /// The handler is never called inline from the initiating thread.
    ///
    /// Per-operation cancellation is not supported.  The request timeout is
    /// set in `req.timeout_ms` and enforced by Rust.
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

    /// Attempt each URL in @p reqs in order; stop as soon as one succeeds
    /// (no error_code).  If all fail, complete with the last error.
    ///
    /// This is a minimal composed operation built on `asyncRequest`.
    ///
    /// TODO: support per-site timeout overrides, early cancellation across
    ///       all in-flight attempts, and structured logging of per-site errors.
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
            // Heap-allocate shared state so the recursive per-site
            // completion lambdas can be copyable.
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
