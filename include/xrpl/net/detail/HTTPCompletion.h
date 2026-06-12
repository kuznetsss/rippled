#pragma once

// This header is included directly by the cxx bridge via `include!()`, which
// places it at the TOP of the generated ffi.h — before RequestResult is
// defined.  We therefore forward-declare RequestResult here rather than
// re-including ffi.h.  At every call-site that actually invokes `complete`
// (the definition of HTTPCompletionImpl::complete in HTTPCompletionImpl.h and
// the cxx call in the Rust task), ffi.h is already fully included so
// RequestResult is complete.
//
// NO guard token here — cxx must be able to include this header directly.

namespace rs::http_client {

// Forward declaration: full definition is in rs_http_client_cxxbridge/ffi.h.
struct RequestResult;

}  // namespace rs::http_client

namespace xrpl::detail {

/// Type-erased base for the per-request completion state.
///
/// cxx exposes this as an opaque C++ type to Rust.  Rust holds a
/// `UniquePtr` to this type and calls `complete()` on it when the HTTP
/// request finishes.  The `UniquePtr` destructor runs the virtual destructor
/// here, cleanly freeing the concrete `HTTPCompletionImpl<Handler>`.
///
/// `complete` posts the handler onto its associated Asio executor but does
/// NOT delete `this` — ownership stays with the caller's `UniquePtr` and is
/// freed on scope exit.
struct HTTPCompletion
{
    virtual ~HTTPCompletion() = default;

    virtual void
    complete(::rs::http_client::RequestResult result) = 0;
};

}  // namespace xrpl::detail
