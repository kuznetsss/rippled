#pragma once

#include <rust/cxx.h>  // rust::Slice

#include <cstdint>

// Shared structs live in the generated header; forward-declare so this header
// (which the generated header includes) doesn't depend on their definitions.
namespace rs::wasm_vm {
struct HashResult;
}

namespace xrpl::wasmrs {

// The host context handed to the Rust engine. For now it forwards the pure
// `sha512_half` to the existing C++ primitive; it will gain ledger access
// (an xrpl::HostFunctions& / ApplyContext) when ledger host-fns are wired.
struct HostContext
{
    rs::wasm_vm::HashResult
    sha512_half(rust::Slice<std::uint8_t const> data) const;
};

}  // namespace xrpl::wasmrs
