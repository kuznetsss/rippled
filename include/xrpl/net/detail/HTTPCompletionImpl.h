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

/** Map a @c RequestError to a @c boost::system::error_code using
 *  @c boost::system::errc values.  Does not introduce a custom error category.
 *
 *  Defined in HTTPClientRust.cpp so the mapping lives in one translation unit.
 */
boost::system::error_code
toErrorCode(::rs::http_client::RequestError code);

/** Completion state for one in-flight request, handed to Rust as an opaque
 *  @c HTTPCompletion.
 *
 *  Stores the decay-copied handler and an executor work guard that keeps the
 *  @c io_context alive while the request is in flight.
 *
 *  The handler's associated executor is type-erased into an @c any_io_executor
 *  rather than a second template parameter, since @c asyncRequest only ever
 *  supplies that fallback.
 *
 *  @tparam Handler  User completion handler, invoked as
 *                   @c handler(error_code, rs::http_client::Response).
 *
 *  @note Per-operation cancellation is unsupported.  The request ends only by
 *        completing normally or via runtime shutdown (which triggers
 *        @c RequestError::Canceled through the Rust @c CompletionGuard).
 */
template <class Handler>
class HTTPCompletionImpl final : public HTTPCompletion
{
private:
    Handler handler_;
    boost::asio::any_io_executor executor_;
    boost::asio::executor_work_guard<boost::asio::any_io_executor> work_;

public:
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
        // Move handler and response off `this` before posting: the Rust
        // UniquePtr destroys `this` as soon as complete() returns.
        auto h = std::move(handler_);
        auto resp = std::move(result.response);
        // Capture the work guard in the lambda so the io_context stays alive
        // until the handler runs; it is released before the handler fires to
        // avoid potential deadlocks on io_context::stop().
        boost::asio::post(
            executor_,
            [h = std::move(h), ec, resp = std::move(resp), work = std::move(work_)]() mutable {
                work.reset();
                h(ec, std::move(resp));
            });
    }
};

}  // namespace xrpl::detail
