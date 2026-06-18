#pragma once

#if !defined(XRPL_NET_HTTPCLIENT_INTERNAL)
#error "include <xrpl/net/HTTPClient.h>, not this detail header directly"
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

/**
 * @brief Concrete HTTPCompletion that resumes an Asio completion handler.
 *
 * Posts the request result to the handler's associated executor (or the
 * fallback executor supplied at construction) and holds an executor work guard
 * so the target io_context stays alive until the result is delivered.
 *
 * @tparam Handler the Asio completion handler type to resume.
 */
template <class Handler>
class HTTPCompletionImpl final : public HTTPCompletion
{
private:
    Handler handler_;
    boost::asio::any_io_executor executor_;
    boost::asio::executor_work_guard<boost::asio::any_io_executor> work_;

public:
    /**
     * @brief Capture the handler and the executor that will run it.
     *
     * @param h completion handler to invoke with the result
     * @param fallback executor used when @p h has no associated executor
     */
    HTTPCompletionImpl(Handler h, boost::asio::any_io_executor fallback)
        : handler_(std::move(h))
        , executor_(boost::asio::get_associated_executor(handler_, fallback))
        , work_(boost::asio::make_work_guard(executor_))
    {
    }

    /**
     * @brief Convert the Rust result to std::expected and post it to the handler.
     */
    void
    complete(::rs::http_client::RequestResult result) override
    {
        std::expected<::rs::http_client::Response, HttpError> expectedResult =
            (result.code == ::rs::http_client::RequestError::Ok)
            ? std::expected<::rs::http_client::Response, HttpError>{std::move(result.response)}
            : std::unexpected(HttpError{result.code, std::string(result.message)});

        // Work guard is released inside the lambda before the handler fires to
        // avoid potential deadlocks on io_context::stop().
        boost::asio::post(
            executor_,
            [handler = std::move(handler_),
             expectedResult = std::move(expectedResult),
             work = std::move(work_)]() mutable {
                work.reset();
                handler(std::move(expectedResult));
            });
    }
};

}  // namespace xrpl::detail
