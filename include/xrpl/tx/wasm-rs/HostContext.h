#pragma once

#include <rust/cxx.h>  // rust::Slice, rust::Str

#include <cstdint>

// Shared structs live in the generated header; forward-declare so this header
// (which the generated header includes) doesn't depend on their definitions.
namespace rs::wasm_vm {
struct Hash;
struct HashResult;
struct BytesResult;
}  // namespace rs::wasm_vm

// Forward-declared rather than including <xrpl/tx/wasm/HostFunc.h>: this
// header is `include!()`'d by the cxxbridge-generated translation unit, which
// builds under the `rs_wasm_vm_cxxbridge` CMake target — it gets only the
// project `include/` dir (see crates/CMakeLists.txt), not the Boost include
// paths that HostFunc.h -> Slice.h -> strHex.h transitively need. A reference
// member and declarations-only don't require a complete type, so the forward
// declaration is enough here; the .cpp files that call through `hf` (compiled
// as part of `xrpl_tests`, which does link Boost) include the full header.
namespace xrpl {
class HostFunctions;
}  // namespace xrpl

namespace xrpl::wasmrs {

// The host context handed to the Rust engine. Wraps a real (or mock)
// `xrpl::HostFunctions` — the single source of truth for ledger access — and
// forwards each host-fn call through it, translating `std::expected` results
// into the plain-data wire types cxx can share across the FFI boundary.
struct HostContext
{
    xrpl::HostFunctions& hf;

    rs::wasm_vm::HashResult
    sha512_half(rust::Slice<std::uint8_t const> data) const;

    std::int64_t
    get_ledger_sqn() const;

    rs::wasm_vm::BytesResult
    get_current_ledger_obj_field(std::int32_t field) const;

    std::int32_t
    trace(rust::Str msg, rust::Slice<std::uint8_t const> data, bool as_hex) const;

    std::int32_t
    trace_num(rust::Str msg, std::int64_t number) const;
};

}  // namespace xrpl::wasmrs
