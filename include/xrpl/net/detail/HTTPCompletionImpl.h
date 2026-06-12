#pragma once

#if !defined(XRPL_NET_HTTPCLIENTRUST_INTERNAL)
#error "include <xrpl/net/HTTPClientRust.h>, not this detail header directly"
#endif

#include <xrpl/net/detail/HTTPCompletion.h>

#include <boost/asio/any_io_executor.hpp>
#include <boost/asio/associated_executor.hpp>
#include <boost/asio/async_result.hpp>
#include <boost/asio/executor_work_guard.hpp>
#include <boost/asio/post.hpp>
#include <boost/system/errc.hpp>
#include <boost/system/error_code.hpp>

#include <rs_http_client_cxxbridge/ffi.h>

namespace xrpl::detail {

/// Map a `RequestError` to a generic `boost::system::error_code` using
/// `boost::system::errc` codes.  Does NOT invent a custom error category.
///
/// Defined in HTTPClientRust.cpp so the mapping is in one translation unit.
boost::system::error_code
toErrorCode(::rs::http_client::RequestError code);

/// Enqueue the request on the Tokio runtime.
///
/// Ownership of `completion` moves into Rust.  On enqueue failure Rust drops
/// the `UniquePtr<HTTPCompletion>` before the task runs, and the Rust
/// `CompletionGuard`'s `Drop` impl calls `complete` with `RequestError::Canceled`
/// — so there is nothing to reclaim on the C++ side.
///
/// Defined in HTTPClientRust.cpp.
void
startRequest(::rs::http_client::Request req, std::unique_ptr<HTTPCompletion> completion);

/// Concrete completion state: stores the moved handler and holds an
/// executor work guard to keep the io_context alive while the request is
/// in flight.
///
/// @tparam Handler  Decay-copy of the user's completion handler; must be
///                  callable as `handler(error_code, rs::http_client::Response)`.
///
/// The fallback executor is always `boost::asio::any_io_executor` (the public
/// API only ever supplies that), so it is not a template parameter.  The
/// handler's associated executor (or the fallback) is type-erased into
/// `executor_`.
///
/// Per-operation cancellation is NOT supported.  The request timeout is
/// owned by the Rust side; the only early-out on the C++ side is runtime
/// shutdown (which triggers the `CompletionGuard` drop in Rust and calls
/// back with `RequestError::Canceled`).
template <class Handler>
struct HTTPCompletionImpl final : HTTPCompletion
{
    HTTPCompletionImpl(Handler h, boost::asio::any_io_executor fallback)
        : handler_(std::move(h))
        , executor_(boost::asio::get_associated_executor(handler_, fallback))
        , work_(boost::asio::make_work_guard(executor_))
    {
    }

    void
    complete(::rs::http_client::RequestResult result) override
    {
        auto ec = toErrorCode(result.code);
        // Move everything we need off `this` before posting, because the
        // UniquePtr<HTTPCompletion> in Rust will destroy `this` on scope exit
        // (immediately after complete() returns).
        auto h = std::move(handler_);
        auto resp = std::move(result.response);
        // Move the work guard INTO the lambda so it keeps the io_context alive
        // until the handler fires, then drops naturally when the lambda returns.
        boost::asio::post(
            executor_,
            [h = std::move(h), ec, resp = std::move(resp), work = std::move(work_)]() mutable {
                work.reset();  // release before handler fires to avoid deadlocks
                h(ec, std::move(resp));
            });
    }

private:
    Handler handler_;
    boost::asio::any_io_executor executor_;
    boost::asio::executor_work_guard<boost::asio::any_io_executor> work_;
};

}  // namespace xrpl::detail
