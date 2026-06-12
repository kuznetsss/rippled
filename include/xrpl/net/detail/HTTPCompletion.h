#pragma once

// This header is included directly by the cxx bridge via `include!()`, which
// places it at the TOP of the generated ffi.h — before RequestResult is
// defined.  Forward-declaring RequestResult here (rather than re-including
// ffi.h) breaks the cycle: at every site that actually calls complete(),
// ffi.h is already fully included and the type is complete.
//
// NO guard token here — cxx must be able to include this header directly.

namespace rs::http_client {

// Forward declaration: full definition is in rs_http_client_cxxbridge/ffi.h.
struct RequestResult;

}  // namespace rs::http_client

namespace xrpl::detail {

/** Type-erased base for per-request completion state, bridged to Rust as an
 *  opaque @c HTTPCompletion.
 *
 *  Rust holds a @c UniquePtr<HTTPCompletion> and calls @c complete() when the
 *  request finishes or is canceled.  The @c UniquePtr destructor invokes the
 *  virtual @c ~HTTPCompletion, which frees the concrete
 *  @c HTTPCompletionImpl<Handler>.
 *
 *  @note @c complete() must NOT delete @c this — ownership belongs to the
 *        Rust @c UniquePtr and is released when that pointer goes out of scope.
 */
struct HTTPCompletion
{
    virtual ~HTTPCompletion() = default;

    virtual void
    complete(::rs::http_client::RequestResult result) = 0;
};

}  // namespace xrpl::detail
