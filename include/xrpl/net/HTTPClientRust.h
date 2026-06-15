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

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

namespace xrpl {

// namespace detail {
//
// template <class Handler>
// struct FailoverState;
//
// }  // namespace detail

class HTTPRequestBuilder
{
private:
    ::rs::http_client::Request request_;
    std::vector<uint8_t> body_;

public:
    static constexpr size_t kDefaultMaxResponseSize = 2 * 1024 * 1024;

    HTTPRequestBuilder(
        std::string_view url,
        ::rs::http_client::HTTPMethod method,
        std::chrono::steady_clock::duration timeout);

    HTTPRequestBuilder&
    addHeader(std::string_view name, std::string_view value);

    HTTPRequestBuilder&
    setBody(std::vector<uint8_t> body);

    HTTPRequestBuilder&
    setMaxResponseSize(size_t size);

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
            ::rs::http_client::http_request(
                std::move(request), bodySlice, std::move(completion));
        };
        return boost::asio::async_initiate<
            CompletionToken,
            void(boost::system::error_code, ::rs::http_client::Response)>(
            std::move(initiation),
            token,
            std::move(executor),
            std::move(request_),
            std::move(body_));
    }

    /*
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
            // Shared ownership lets the per-site completion lambdas be copyable.
            auto state = std::make_shared<detail::FailoverState<HandlerType>>(
                std::move(executor), std::move(requests), std::move(handler));
            state->next(state);
        };
        return boost::asio::async_initiate<
            CompletionToken,
            void(boost::system::error_code, ::rs::http_client::Response)>(
            std::move(initiation), token, std::move(executor), std::move(requests));
    }
    */
};

}  // namespace xrpl

// #include <xrpl/net/detail/FailoverState.ipp>

#undef XRPL_NET_HTTPCLIENTRUST_INTERNAL
