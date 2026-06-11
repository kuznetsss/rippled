#pragma once

// This header is included by the generated cxx bridge header
// (rs_http_client_cxxbridge/ffi.h) via cxx's `include!()` directive, which
// places this include at the TOP of ffi.h — before RequestResult is defined.
// We therefore use a forward declaration here rather than re-including ffi.h.
// At the only call-site that matters (the definition of resume_http_request in
// HTTPClientRust.cpp), ffi.h is already fully included so RequestResult is
// complete.
#include <cstddef>

namespace rs::http_client {

// Forward declaration: full definition is in rs_http_client_cxxbridge/ffi.h.
struct RequestResult;

/// Called by Rust (from a Tokio worker thread) when an HTTP request completes.
///
/// The @p completion token is a type-erased `reinterpret_cast<std::size_t>`
/// of a heap-allocated `xrpl::detail::HttpCompletion` object. This function
/// reclaims the pointer, posts the stored C++ handler onto its associated
/// executor, and frees the State — completing the lifecycle exactly once.
///
/// @note This function is declared here so the cxx bridge can call it as an
///       extern "C++" symbol. The single-line definition lives in
///       src/libxrpl/net/HTTPClientRust.cpp to keep the cxx bridge namespace
///       consistent with the linker symbol.
void
resume_http_request(::std::size_t completion, ::rs::http_client::RequestResult result);

}  // namespace rs::http_client
