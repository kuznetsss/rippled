#include <xrpl/net/HTTPClientRust.h>

#include <boost/system/errc.hpp>
#include <boost/system/error_code.hpp>

#include <chrono>
#include <cstdint>
#include <span>
#include <string_view>
#include <utility>

namespace xrpl {

HTTPRequestBuilder::HTTPRequestBuilder(std::string_view url, ::rs::http_client::HTTPMethod method)
{
    request_.method = method;
    request_.url = rust::String(url.data(), url.size());
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
HTTPRequestBuilder::setBody(std::span<uint8_t const> body)
{
    rust::Vec<uint8_t> vec;
    vec.reserve(body.size());
    for (uint8_t b : body)
        vec.push_back(b);
    request_.body = std::move(vec);
    return *this;
}

HTTPRequestBuilder&
HTTPRequestBuilder::setTimeout(std::chrono::steady_clock::duration timeout)
{
    request_.timeout_ms = static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::milliseconds>(timeout).count());
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
        case ::rs::http_client::RequestError::NotInitialized:
            return errc::make_error_code(errc::operation_not_permitted);
    }
    return errc::make_error_code(errc::operation_canceled);
}

}  // namespace detail

}  // namespace xrpl
