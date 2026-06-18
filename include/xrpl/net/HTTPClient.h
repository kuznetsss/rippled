#pragma once

#include <xrpl/beast/utility/Journal.h>

#include <boost/asio/any_io_executor.hpp>
#include <boost/asio/async_result.hpp>

#include <rs_http_client_cxxbridge/ffi.h>

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <expected>
#include <memory>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

#define XRPL_NET_HTTPCLIENT_INTERNAL
#include <xrpl/net/detail/HTTPCompletionImpl.h>

namespace xrpl {

/**
 * @brief Initialise the global HTTP client: the Tokio runtime and TLS context.
 *
 * Starts the shared Tokio runtime with @p numThreads worker threads and builds
 * the reqwest/TLS context used by every request. Safe to call more than once:
 * an already-initialised runtime is treated as success, so test processes that
 * construct many environments do not fail on subsequent calls. Re-initialising
 * replaces the TLS context with one built from the supplied verification
 * settings.
 *
 * @param numThreads number of Tokio worker threads to spawn
 * @param sslVerify whether to verify server certificates and host names
 * @param sslVerifyFile path to a PEM bundle that replaces the default CA roots,
 *                      or empty to keep them
 * @param sslVerifyDir path to a directory of PEM certificates to trust in
 *                     addition to the active roots, or empty
 * @param j journal for logging
 *
 * @throws std::runtime_error if the runtime or TLS context cannot be created.
 */
void
initHTTPClient(
    std::size_t numThreads,
    bool sslVerify,
    std::string const& sslVerifyFile,
    std::string const& sslVerifyDir,
    beast::Journal j);

/**
 * @brief Tear down the global Tokio runtime and TLS context.
 *
 * Intended to run once at process shutdown. Never throws — it is invoked from
 * RAII destructors (see HTTPClientShutdownGuard); any error is logged only.
 *
 * @param timeout how long to wait for in-flight requests to drain before the
 *                runtime is forced down
 */
void
shutdownHTTPClient(std::chrono::milliseconds timeout = std::chrono::milliseconds{2000});

/**
 * @brief RAII guard that calls shutdownHTTPClient() when it goes out of scope.
 *
 * Declare one after initHTTPClient() so the runtime and TLS context are torn
 * down on every exit path.
 */
struct HTTPClientShutdownGuard
{
    ~HTTPClientShutdownGuard()
    {
        xrpl::shutdownHTTPClient();
    }
};

/**
 * @brief Error details for a failed HTTP request.
 *
 * Delivered as the error alternative of a request completion's std::expected.
 * `code` is a coarse category from the Rust client; `message` carries the
 * flattened reqwest cause chain.
 */
struct HttpError
{
    ::rs::http_client::RequestError code;
    std::string message;
};

/**
 * @brief Builds and submits a single asynchronous HTTP request.
 *
 * Configure the request fluently with addHeader(), setBody() and
 * setMaxResponseSize(), then issue it with asyncSubmit(). The request runs on
 * the global Tokio runtime (see initHTTPClient()) and the result is delivered
 * to the completion token's associated executor.
 */
class HTTPRequestBuilder
{
private:
    ::rs::http_client::Request request_;
    std::vector<uint8_t> body_;

public:
    /** Default response-body cap (2 MiB) unless setMaxResponseSize() overrides it. */
    static constexpr size_t kDefaultMaxResponseSize = 2 * 1024 * 1024;

    /**
     * @brief Construct a request for @p url using @p method.
     *
     * @param url absolute request URL (http or https)
     * @param method HTTP method to use
     * @param timeout overall request timeout
     */
    HTTPRequestBuilder(
        std::string_view url,
        ::rs::http_client::HTTPMethod method,
        std::chrono::steady_clock::duration timeout);

    /**
     * @brief Append a request header.
     *
     * @param name header field name
     * @param value header field value
     * @return reference to this builder, for chaining
     */
    HTTPRequestBuilder&
    addHeader(std::string_view name, std::string_view value);

    /**
     * @brief Set the request body.
     *
     * @param body bytes to send; moved into the request
     * @return reference to this builder, for chaining
     */
    HTTPRequestBuilder&
    setBody(std::vector<uint8_t> body);

    /**
     * @brief Override the maximum response body size accepted.
     *
     * A response larger than this fails with a "too large" error rather than
     * being buffered.
     *
     * @param size maximum number of body bytes to accept
     * @return reference to this builder, for chaining
     */
    HTTPRequestBuilder&
    setMaxResponseSize(size_t size);

    /**
     * @brief Asynchronously issue the configured request.
     *
     * Hands the request to the global Tokio runtime and completes once a
     * response arrives or the request fails. The result is posted to the
     * completion handler's associated executor (falling back to @p executor),
     * so the handler runs on the caller's io_context; an executor work guard
     * keeps that context alive until completion.
     *
     * @tparam CompletionToken Asio completion token type (plain callback,
     *                         use_future, use_awaitable, yield_context, ...)
     * @param executor fallback executor used when the handler has no associated
     *                 executor of its own
     * @param token completion token invoked with
     *              std::expected<::rs::http_client::Response, HttpError>
     * @return whatever @p token yields (e.g. void for a callback, a future for
     *         use_future)
     */
    template <class CompletionToken>
    auto
    asyncSubmit(boost::asio::any_io_executor executor, CompletionToken&& token)
    {
        auto initiation = [](auto handler,
                             boost::asio::any_io_executor executor,
                             ::rs::http_client::Request request,
                             std::vector<uint8_t> body) {
            using HandlerType = std::decay_t<decltype(handler)>;
            std::unique_ptr<detail::HTTPCompletion> completion =
                std::make_unique<detail::HTTPCompletionImpl<HandlerType>>(
                    std::move(handler), std::move(executor));
            // http_request copies the slice synchronously (Rust-side to_vec)
            // before returning, so `body` outliving this call is sufficient.
            // An empty vector's data() may be null; pass a default (non-null,
            // zero-length) slice in that case to avoid UB when cxx rebuilds the
            // Rust &[u8].
            rust::Slice<uint8_t const> bodySlice = body.empty()
                ? rust::Slice<uint8_t const>{}
                : rust::Slice<uint8_t const>(body.data(), body.size());
            ::rs::http_client::http_request(std::move(request), bodySlice, std::move(completion));
        };
        return boost::asio::async_initiate<
            CompletionToken,
            void(std::expected<::rs::http_client::Response, HttpError>)>(
            std::move(initiation),
            token,
            std::move(executor),
            std::move(request_),
            std::move(body_));
    }
};

}  // namespace xrpl

#undef XRPL_NET_HTTPCLIENT_INTERNAL
