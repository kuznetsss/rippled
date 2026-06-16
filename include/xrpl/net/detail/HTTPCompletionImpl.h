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

#include <rs_http_client_cxxbridge/ffi.h>

#include <expected>
#include <utility>

namespace xrpl::detail {

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
        std::expected<::rs::http_client::Response, HttpError> expectedResult =
            (result.code == ::rs::http_client::RequestError::Ok)
            ? std::expected<::rs::http_client::Response, HttpError>{std::move(result.response)}
            : std::unexpected(HttpError{result.code, std::string(result.message)});

        // Move handler and result off `this` before posting: the Rust
        // UniquePtr destroys `this` as soon as complete() returns.
        auto h = std::move(handler_);
        // Work guard is released inside the lambda before the handler fires to
        // avoid potential deadlocks on io_context::stop().
        boost::asio::post(
            executor_,
            [h = std::move(h),
             expectedResult = std::move(expectedResult),
             work = std::move(work_)]() mutable {
                work.reset();
                h(std::move(expectedResult));
            });
    }
};

}  // namespace xrpl::detail
