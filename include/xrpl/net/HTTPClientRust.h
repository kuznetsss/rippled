#pragma once

// The generated cxx bridge header exposes http_client::Request, Response, etc.
// It also transitively includes HttpClientCallback.h.
#include <boost/asio/any_io_executor.hpp>
#include <boost/asio/associated_executor.hpp>
#include <boost/asio/async_result.hpp>
#include <boost/asio/executor_work_guard.hpp>
#include <boost/asio/post.hpp>
#include <boost/system/errc.hpp>
#include <boost/system/error_code.hpp>

#include <rs_http_client_cxxbridge/ffi.h>

#include <memory>
#include <type_traits>
#include <utility>
#include <vector>

namespace xrpl {

namespace detail {

/// Non-template base used by `resume_http_request` so the callback shim can
/// call `complete` without knowing the concrete handler type.
///
/// @note `complete` posts the handler onto its associated executor but does
///       NOT delete `this`.  The owning `unique_ptr<HttpCompletion>` is held
///       on the stack in `resume_http_request` and freed on scope exit.
struct HttpCompletion
{
    virtual ~HttpCompletion() = default;

    virtual void
    complete(::http_client::RequestResult result) = 0;
};

/// Map a `RequestError` to a generic `boost::system::error_code` using
/// `boost::system::errc` codes.  Does NOT invent a custom error category.
///
/// Defined in HTTPClientRust.cpp so the mapping is in one translation unit.
boost::system::error_code
toErrorCode(::http_client::RequestError code);

/// Enqueue the request on the Tokio runtime.
///
/// On success (`Status.code == ErrorCode::Ok`) ownership of `c` is
/// transferred to Rust via the `completion` token; the caller must call
/// `c.release()` first.  On failure C++ retains ownership of `c`, calls
/// `c->complete(...)` with an appropriate error, and lets `c` be destroyed.
///
/// Defined in HTTPClientRust.cpp.
void
startRequest(::http_client::Request req, std::unique_ptr<HttpCompletion> c);

/// Concrete completion state: stores the moved handler and holds an
/// executor work guard to keep the io_context alive while the request is
/// in flight.
///
/// @tparam Handler  Decay-copy of the user's completion handler; must be
///                  callable as `handler(error_code, http_client::Response)`.
/// @tparam Executor Fallback executor used when the handler has no associated
///                  executor of its own.
///
/// Per-operation cancellation is NOT supported.  The request timeout is
/// owned by the Rust side; the only early-out on the C++ side is runtime
/// shutdown (which triggers the Drop guard in Rust and calls back with
/// RequestError::Canceled).
template <class Handler, class Executor>
struct HttpCompletionImpl final : HttpCompletion
{
    HttpCompletionImpl(Handler h, Executor fallback)
        : handler_(std::move(h))
        , executor_(boost::asio::get_associated_executor(handler_, fallback))
        , work_(boost::asio::make_work_guard(executor_))
    {
    }

    void
    complete(::http_client::RequestResult result) override
    {
        auto ec = toErrorCode(result.code);
        // Move everything we need off `this` before posting, because the
        // unique_ptr in resume_http_request will destroy `this` on scope exit
        // (immediately after complete() returns).
        auto h = std::move(handler_);
        auto resp = std::move(result.response);
        // Move the work guard INTO the lambda so it keeps the io_context alive
        // until the handler fires, then drops naturally when the lambda returns.
        boost::asio::post(
            executor_,
            [h = std::move(h), ec, resp = std::move(resp), work = std::move(work_)]() mutable {
                work.reset();  // release before handler fires to avoid deadlocks
                h(ec, std::move(resp));
            });
    }

private:
    Handler handler_;
    Executor executor_;
    boost::asio::executor_work_guard<Executor> work_;
};

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
    std::vector<::http_client::Request> reqs;
    std::size_t index{0};
    Handler handler;

    FailoverState(boost::asio::any_io_executor e, std::vector<::http_client::Request> r, Handler h)
        : ex(std::move(e)), reqs(std::move(r)), handler(std::move(h))
    {
    }

    void
    next(std::shared_ptr<FailoverState> self);
};

}  // namespace detail

/// Asio-compatible async HTTP client backed by a Tokio runtime in Rust.
///
/// Exposes a single-URL `asyncRequest` that composes with any Asio
/// completion token (lambda, `use_awaitable`, `use_future`, …), and a
/// multi-site failover helper that chains `asyncRequest` calls until one
/// succeeds or the list is exhausted.
class HTTPClientRust
{
public:
    /// Issue a single HTTP request and complete with
    /// `void(boost::system::error_code, http_client::Response)`.
    ///
    /// The handler is always invoked on the executor associated with the
    /// completion token (falling back to @p ex when the token has none).
    /// The handler is never called inline from the initiating thread.
    ///
    /// Per-operation cancellation is not supported.  The request timeout is
    /// set in `req.timeout_ms` and enforced by Rust.
    template <class CompletionToken>
    static auto
    asyncRequest(
        boost::asio::any_io_executor ex,
        ::http_client::Request req,
        CompletionToken&& token)
    {
        return boost::asio::async_initiate<
            CompletionToken,
            void(boost::system::error_code, ::http_client::Response)>(
            [](auto handler, boost::asio::any_io_executor ex, ::http_client::Request req) {
                using H = std::decay_t<decltype(handler)>;
                using E = boost::asio::any_io_executor;
                auto c = std::make_unique<detail::HttpCompletionImpl<H, E>>(
                    std::move(handler), std::move(ex));
                detail::startRequest(std::move(req), std::move(c));
            },
            token,
            std::move(ex),
            std::move(req));
    }

    /// Attempt each URL in @p reqs in order; stop as soon as one succeeds
    /// (no error_code).  If all fail, complete with the last error.
    ///
    /// This is a minimal composed operation built on `asyncRequest`.
    ///
    /// TODO: support per-site timeout overrides, early cancellation across
    ///       all in-flight attempts, and structured logging of per-site errors.
    template <class CompletionToken>
    static auto
    asyncRequestAny(
        boost::asio::any_io_executor ex,
        std::vector<::http_client::Request> reqs,
        CompletionToken&& token)
    {
        return boost::asio::async_initiate<
            CompletionToken,
            void(boost::system::error_code, ::http_client::Response)>(
            [](auto handler,
               boost::asio::any_io_executor ex,
               std::vector<::http_client::Request> reqs) {
                using H = std::decay_t<decltype(handler)>;
                // Heap-allocate shared state so the recursive per-site
                // completion lambdas can be copyable.
                auto state = std::make_shared<detail::FailoverState<H>>(
                    std::move(ex), std::move(reqs), std::move(handler));
                state->next(state);
            },
            token,
            std::move(ex),
            std::move(reqs));
    }
};

}  // namespace xrpl

// ---------------------------------------------------------------------------
// Out-of-line definition of FailoverState::next — must come after
// HTTPClientRust is fully defined because it calls HTTPClientRust::asyncRequest.
// ---------------------------------------------------------------------------
namespace xrpl::detail {

template <class Handler>
void
FailoverState<Handler>::next(std::shared_ptr<FailoverState<Handler>> self)
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
                ::http_client::Response{});
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
            boost::system::error_code ec, ::http_client::Response resp) mutable {
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

}  // namespace xrpl::detail
