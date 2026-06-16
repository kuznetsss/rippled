#include <xrpl/net/HTTPClientRust.h>

#include <boost/system/errc.hpp>
#include <boost/system/error_code.hpp>

#include <chrono>
#include <cstdint>
#include <string_view>
#include <utility>

namespace xrpl {

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

namespace detail {

boost::system::error_code
toErrorCode(::rs::http_client::RequestError code)
{
    namespace errc = boost::system::errc;

    switch (code)
    {
        case ::rs::http_client::RequestError::Ok:
            return {};
        case ::rs::http_client::RequestError::Timeout:
            return errc::make_error_code(errc::timed_out);
        case ::rs::http_client::RequestError::Failed:
            return errc::make_error_code(errc::io_error);
        case ::rs::http_client::RequestError::TooLarge:
            return errc::make_error_code(errc::value_too_large);
        case ::rs::http_client::RequestError::Canceled:
            return errc::make_error_code(errc::operation_canceled);
        case ::rs::http_client::RequestError::NotInitialized:
            return errc::make_error_code(errc::operation_not_permitted);
    }
    return errc::make_error_code(errc::operation_canceled);
}

}  // namespace detail

}  // namespace xrpl
