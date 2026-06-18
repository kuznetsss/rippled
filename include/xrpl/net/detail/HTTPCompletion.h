#pragma once

// This header is included directly by the cxx bridge via `include!()`, which
// places it at the TOP of the generated ffi.h — before RequestResult is
// defined.  Forward-declaring RequestResult here breaks the cycle; at every
// call site ffi.h is already fully included and the type is complete.
//
// NO guard token here — cxx must be able to include this header directly.

namespace rs::http_client {

// Forward declaration: full definition is in rs_http_client_cxxbridge/ffi.h.
struct RequestResult;

}  // namespace rs::http_client

namespace xrpl::detail {

/**
 * @brief Type-erased base for delivering an HTTP request result back to C++.
 *
 * The Rust client owns one of these as a cxx `UniquePtr` for the lifetime of a
 * request and calls complete() exactly once when the request finishes.
 */
struct HTTPCompletion
{
    virtual ~HTTPCompletion() = default;

    /**
     * @brief Deliver the request result.
     *
     * @note Must not delete `this` — ownership belongs to the Rust UniquePtr.
     *
     * @param result outcome of the request (a response or an error)
     */
    virtual void
    complete(::rs::http_client::RequestResult result) = 0;
};

}  // namespace xrpl::detail
