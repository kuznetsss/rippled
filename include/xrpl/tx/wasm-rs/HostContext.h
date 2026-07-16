#pragma once

#include <rust/cxx.h>  // rust::Slice, rust::Str

#include <cstdint>

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

    // The value-producing calls take `out`, a mutable slice aliasing the
    // guest's output region in wasm linear memory: they write the value's
    // bytes straight into it (the single copy) and return the value's true
    // length (>= 0), or a negative `HostError` code. The engine owns the
    // buffer-fit / field-cap / transfer policy, deriving it from that length.
    std::int32_t
    sha512_half(rust::Slice<std::uint8_t const> data, rust::Slice<std::uint8_t> out) const;

    std::int32_t
    get_ledger_sqn(rust::Slice<std::uint8_t> out) const;

    std::int32_t
    get_current_ledger_obj_field(std::int32_t field, rust::Slice<std::uint8_t> out) const;

    std::int32_t
    trace(rust::Str msg, rust::Slice<std::uint8_t const> data, bool as_hex) const;

    std::int32_t
    trace_num(rust::Str msg, std::int64_t number) const;
};

}  // namespace xrpl::wasmrs
