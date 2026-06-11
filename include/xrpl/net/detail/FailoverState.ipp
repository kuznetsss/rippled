#pragma once

#if !defined(XRPL_NET_HTTPCLIENTRUST_INTERNAL)
#error "include <xrpl/net/HTTPClientRust.h>, not this detail header directly"
#endif

#include <xrpl/net/HTTPClientRust.h>

#include <boost/asio/any_io_executor.hpp>
#include <boost/asio/post.hpp>

#include <rs_http_client_cxxbridge/ffi.h>

#include <memory>

namespace xrpl::detail {

/// Shared state for the multi-site failover chain.
///
/// Defined in xrpl::detail so it is visible inside the lambda passed to
/// async_initiate (lambdas cannot access private members of the enclosing
/// class template).
///
/// @tparam Handler  Decay-copy of the user's completion handler.
template <class Handler>
struct FailoverState
{
    boost::asio::any_io_executor ex;
    std::vector<::rs::http_client::Request> reqs;
    std::size_t index{0};
    Handler handler;

    FailoverState(
        boost::asio::any_io_executor e,
        std::vector<::rs::http_client::Request> r,
        Handler h)
        : ex(std::move(e)), reqs(std::move(r)), handler(std::move(h))
    {
    }

    void
    next(std::shared_ptr<FailoverState> self)
    {
        if (index >= reqs.size())
        {
            // Only reached when the request list was empty: the per-site lambda
            // below completes on the last attempt, so a non-empty list never
            // falls through here. Post an error so the handler still fires exactly
            // once rather than the operation hanging forever.
            boost::asio::post(ex, [self = std::move(self)]() mutable {
                self->handler(
                    boost::system::errc::make_error_code(boost::system::errc::invalid_argument),
                    ::rs::http_client::Response{});
            });
            return;
        }
        auto req = reqs[index];
        ++index;
        auto idx = index;  // capture current index for the lambda
        HTTPClientRust::asyncRequest(
            ex,
            std::move(req),
            [self = std::move(self), idx](
                boost::system::error_code ec, ::rs::http_client::Response resp) mutable {
                if (!ec || idx >= self->reqs.size())
                {
                    // Success, or last site was just tried — complete.
                    self->handler(ec, std::move(resp));
                }
                else
                {
                    // Try next site.
                    self->next(std::move(self));
                }
            });
    }
};

}  // namespace xrpl::detail
