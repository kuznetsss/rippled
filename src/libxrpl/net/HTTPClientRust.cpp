#include <xrpl/net/HTTPClientRust.h>

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

}  // namespace xrpl
