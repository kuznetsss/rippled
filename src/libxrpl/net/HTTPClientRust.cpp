#include <xrpl/net/HTTPClientRust.h>

#include <xrpl/basics/Log.h>
#include <xrpl/basics/contract.h>
#include <xrpl/beast/utility/Journal.h>

#include <chrono>
#include <cstdint>
#include <cstddef>
#include <string>
#include <string_view>
#include <utility>

namespace xrpl {

void
initHTTPClient(
    std::size_t numThreads,
    bool sslVerify,
    std::string const& sslVerifyFile,
    std::string const& sslVerifyDir,
    beast::Journal j)
{
    // Initialise the Tokio runtime.  AlreadyInitialized is tolerated so that
    // test processes that construct many environments don't fail on subsequent
    // calls.
    auto rtStatus = ::rs::http_client::init_tokio_runtime(numThreads);
    if (rtStatus.code != ::rs::http_client::ErrorCode::Ok &&
        rtStatus.code != ::rs::http_client::ErrorCode::AlreadyInitialized)
    {
        Throw<std::runtime_error>(std::string(rtStatus.message));
    }

    if (rtStatus.code == ::rs::http_client::ErrorCode::AlreadyInitialized)
    {
        JLOG(j.debug()) << "initHTTPClient: Tokio runtime already initialized";
    }
    else
    {
        JLOG(j.debug()) << "initHTTPClient: Tokio runtime initialized";
    }

    // Initialise the TLS context.
    ::rs::http_client::TlsConfig tlsConfig;
    tlsConfig.verify = sslVerify;
    tlsConfig.verify_file = rust::String(sslVerifyFile);
    tlsConfig.verify_dir = rust::String(sslVerifyDir);

    // Re-initialising the TLS context overwrites the previous reqwest client
    // with the supplied config; AlreadyInitialized is tolerated for symmetry
    // with the runtime path above.
    auto tlsStatus = ::rs::http_client::init_tls_context(std::move(tlsConfig));
    if (tlsStatus.code != ::rs::http_client::ErrorCode::Ok &&
        tlsStatus.code != ::rs::http_client::ErrorCode::AlreadyInitialized)
    {
        Throw<std::runtime_error>(std::string(tlsStatus.message));
    }

    JLOG(j.debug()) << "initHTTPClient: TLS context initialized";
}

void
shutdownHTTPClient(std::chrono::milliseconds timeout)
{
    auto rtStatus = ::rs::http_client::shutdown_tokio_runtime(
        static_cast<uint64_t>(timeout.count()));
    (void)rtStatus;  // ignore errors — must not throw from RAII

    auto tlsStatus = ::rs::http_client::reset_tls_context();
    (void)tlsStatus;  // ignore errors — must not throw from RAII
}

HTTPRequestBuilder::HTTPRequestBuilder(
    std::string_view url,
    ::rs::http_client::HTTPMethod method,
    std::chrono::steady_clock::duration timeout)
{
    request_.method = method;
    request_.url = rust::String(url.data(), url.size());
    request_.timeout_ms = static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::milliseconds>(timeout).count());
    request_.max_response_bytes = kDefaultMaxResponseSize;
}

HTTPRequestBuilder&
HTTPRequestBuilder::addHeader(std::string_view name, std::string_view value)
{
    request_.headers.push_back(
        ::rs::http_client::HTTPHeader{
            .name = rust::String(name.data(), name.size()),
            .value = rust::String(value.data(), value.size())});
    return *this;
}

HTTPRequestBuilder&
HTTPRequestBuilder::setBody(std::vector<uint8_t> body)
{
    body_ = std::move(body);
    return *this;
}

HTTPRequestBuilder&
HTTPRequestBuilder::setMaxResponseSize(size_t size)
{
    request_.max_response_bytes = size;
    return *this;
}

}  // namespace xrpl
