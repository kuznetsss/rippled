#pragma once

// Thin C++ front for the Rust `wasm_vm` engine, reached over cxx. This is the
// coarse, once-per-escrow-finish entry (C++ -> Rust); it mirrors the shape of
// the existing `runEscrowWasm` while delegating execution to the Rust engine.
// `runEscrowWasmRs`/`runEscrowWasmRsFromWat` run against a built-in Rust mock
// host; `runEscrowWasmRsFromWatWithCxxHost` instead services host calls via a
// C++ `xrpl::HostFunctions&` (real or mock), forwarded through `HostContext`.

#include <rs_wasm_vm_cxxbridge/ffi.h>

#include <xrpl/tx/wasm-rs/HostContext.h>
#include <xrpl/tx/wasm/HostFunc.h>  // xrpl::HostFunctions (complete type; HostContext.h only forward-declares it)

#include <cstdint>
#include <string_view>
#include <vector>

namespace xrpl::wasmrs {

/// Result of running an escrow contract through the Rust engine.
struct EscrowRunResult
{
    std::int32_t result;    // the `finish` export's return value
    std::uint64_t fuelUsed; // gas consumed (guest + host calls)
};

/// Run `code`'s `finish` export with `gas` fuel through the Rust wasm engine,
/// serviced by the built-in mock host. Throws `rust::Error` if the engine
/// fails to compile/instantiate/run the module.
inline EscrowRunResult
runEscrowWasmRs(std::vector<std::uint8_t> const& code, std::uint64_t gas)
{
    auto const r = rs::wasm_vm::run_escrow_mocked(
        rust::Slice<std::uint8_t const>(code.data(), code.size()), gas);
    return {r.result, r.fuel_used};
}

// Compile `wat` (WebAssembly text) and run its `finish` export through the
// Rust engine with `gas` fuel. Throws `rust::Error` if the WAT is invalid or
// the module fails to run.
inline EscrowRunResult
runEscrowWasmRsFromWat(std::string_view wat, std::uint64_t gas)
{
    auto const code = rs::wasm_vm::compile_wat(rust::Str(wat.data(), wat.size()));
    return runEscrowWasmRs(
        std::vector<std::uint8_t>(code.begin(), code.end()), gas);
}

// Run `wat` through the Rust engine, servicing host calls via a C++ HostContext
// wrapping `hf` (which forwards to real, or mock, xrpl primitives). Proves the
// Rust->C++ callback path.
inline EscrowRunResult
runEscrowWasmRsFromWatWithCxxHost(
    std::string_view wat,
    xrpl::HostFunctions& hf,
    std::uint64_t gas)
{
    auto const code = rs::wasm_vm::compile_wat(rust::Str(wat.data(), wat.size()));
    HostContext ctx{hf};
    auto const r = rs::wasm_vm::run_escrow_with_cxx_host(
        ctx,
        rust::Slice<std::uint8_t const>(code.data(), code.size()),
        gas);
    return {r.result, r.fuel_used};
}

}  // namespace xrpl::wasmrs
