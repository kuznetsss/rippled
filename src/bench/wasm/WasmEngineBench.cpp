//------------------------------------------------------------------------------
/*
    Head-to-head Google Benchmark harness for the two wasm-VM engines
    described in CLAUDE.md's "WASM-VM Rust redesign PoC" section:

      - the existing C++ engine (wasmi C-API), reached via
        `xrpl::runEscrowWasm` (include/xrpl/tx/wasm/WasmVM.h);
      - the Rust-native wasmi engine, reached over cxx via
        `xrpl::wasmrs::runEscrowWasmRsWithCxxHost`
        (include/xrpl/tx/wasm-rs/WasmVmRs.h).

    Measurement model: both engines are driven, per timed iteration, through
    their REAL production entry point on a PRE-ASSEMBLED `.wasm` binary.
    Assembling WAT -> bytes happens ONCE per contract, untimed (via the Rust
    `compile_wat` tooling helper, reached from either engine's benchmark since
    both consume the identical `.wasm` bytes). Each timed iteration is a full
    "finish" (validate + instantiate + lazy-translate + execute) -- there is
    intentionally no prepare/warm-up/persistent-instance split, since that is
    the real per-escrow cost being compared.

    Both engines are driven by the SAME `xrpl::HostFunctions` mock instance
    (`FakeHost` below) with trivial constant returns, so what's being measured
    is engine + binding overhead, not primitive (e.g. real SHA-512) cost.

    Fairness note: the C++ engine normally registers ~60 host-function
    imports (see src/libxrpl/tx/wasm/WasmVM.cpp); for this benchmark that file
    has been trimmed (under `// BENCH: disabled ...` markers) to register only
    the 5 host functions the Rust PoC engine implements, since import
    resolution happens inside the timed region for both engines.
*/
//==============================================================================

#include <benchmark/benchmark.h>

#include <rs_wasm_vm_cxxbridge/ffi.h>

#include <xrpl/basics/Slice.h>
#include <xrpl/protocol/SField.h>
#include <xrpl/tx/wasm-rs/WasmVmRs.h>
#include <xrpl/tx/wasm/HostFunc.h>
#include <xrpl/tx/wasm/WasmCommon.h>
#include <xrpl/tx/wasm/WasmVM.h>

#include <cstdint>
#include <expected>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

namespace {

using namespace xrpl;

// Gas/fuel ceiling handed to both engines. Generous relative to the work
// done per call so it never becomes the limiting factor; what's measured is
// wall-clock time, not gas consumption.
constexpr std::uint64_t GAS = 1'000'000'000ull;

// Minimal C++ mock of `xrpl::HostFunctions` with trivial constant returns --
// the whole point is to measure engine + binding overhead, not the cost of
// any real primitive (ledger lookups, SHA-512, etc). Shared by both engines:
// the Rust engine reaches it via `HostContext` (include/xrpl/tx/wasm-rs/HostContext.h),
// the C++ engine calls it directly through the `xrpl::HostFunctions` vtable.
struct FakeHost : HostFunctions
{
    std::expected<std::uint32_t, HostFunctionError>
    getLedgerSqn() const override
    {
        return 42u;
    }

    std::expected<Bytes, HostFunctionError>
    getCurrentLedgerObjField(SField const&) const override
    {
        return Bytes{0xAA, 0xBB, 0xCC, 0xDD};
    }

    std::expected<Hash, HostFunctionError>
    computeSha512HalfHash(Slice const&) const override
    {
        // Deliberately not a real digest -- constant-cost stand-in so the
        // benchmark measures marshaling/engine overhead, not hashing cost.
        return Hash{};
    }

    std::expected<int32_t, HostFunctionError>
    trace(std::string_view const&, Slice const&, bool) const override
    {
        return 0;
    }

    std::expected<int32_t, HostFunctionError>
    traceNum(std::string_view const&, int64_t) const override
    {
        return 0;
    }
};

// Assembles WAT (WebAssembly text) to bytes ONCE via the Rust engine's
// `compile_wat` tooling helper (crates/wasm_vm/src/ffi.rs). Both engines
// consume the resulting bytes directly -- assembly happens outside any timed
// region.
std::vector<std::uint8_t>
assemble(std::string const& wat)
{
    auto v = rs::wasm_vm::compile_wat(rust::Str(wat.data(), wat.size()));
    return std::vector<std::uint8_t>(v.begin(), v.end());
}

// Builds a `finish` export that calls `callExpr` (a `(call $f ...)`
// expression dropping its result) K times in a loop, then returns 0. Only
// core wasm MVP ops are used (locals, block/loop/br_if, i32.add/ge_u/const,
// i64.const, drop) -- the engine disables bulk-memory / sign-ext /
// multi-value / reference-types / non-trapping-fptoint / extended-const, so
// the generated text must not stray outside the MVP subset.
std::string
makeLoopWat(
    std::string const& importDecl,
    std::string const& dataSection,
    std::string const& callExpr,
    std::uint64_t k)
{
    std::ostringstream oss;
    // clang-format off
    oss << "(module\n"
        << "  " << importDecl << "\n"
        << "  (memory (export \"memory\") 1)\n"
        << "  " << dataSection << "\n"
        << "  (func (export \"finish\") (result i32)\n"
        << "    (local $i i32)\n"
        << "    (block $done\n"
        << "      (loop $loop\n"
        << "        (br_if $done (i32.ge_u (local.get $i) (i32.const " << k << ")))\n"
        << "        (drop " << callExpr << ")\n"
        << "        (local.set $i (i32.add (local.get $i) (i32.const 1)))\n"
        << "        (br $loop)))\n"
        << "    (i32.const 0)))\n";
    // clang-format on
    return oss.str();
}

// A contract with no imports/host calls at all -- the "engine launch" floor
// (validate + instantiate + execute a trivial body), with no host-binding
// cost mixed in.
constexpr std::string_view kNoOpWat =
    R"wat((module (func (export "finish") (result i32) (i32.const 0))))wat";

// One WAT generator per host function under test; each takes the per-run
// loop count K and produces a self-contained module. Signatures/semantics
// mirror the table in CLAUDE.md's benchmark task (identical on both engines).

std::string
ldgrIndexWat(std::uint64_t k)
{
    return makeLoopWat(
        R"((import "host" "ldgr_index" (func $f (param i32 i32) (result i32))))",
        "",
        "(call $f (i32.const 0) (i32.const 4))",
        k);
}

std::string
homeLeFieldWat(std::uint64_t k)
{
    // sfFlags is a real, known field code -- obtained at runtime rather than
    // hardcoded so this doesn't depend on the type/index encoding staying
    // stable. The C++ wrapper decodes the code into an SField before it ever
    // reaches FakeHost, so an invalid code would error before FakeHost sees it.
    return makeLoopWat(
        R"((import "host" "home_le_field" (func $f (param i32 i32 i32) (result i32))))",
        "",
        "(call $f (i32.const " + std::to_string(xrpl::sfFlags.getCode()) +
            ") (i32.const 0) (i32.const 16))",
        k);
}

std::string
sha512HalfWat(std::uint64_t k)
{
    return makeLoopWat(
        R"((import "host" "sha512_half" (func $f (param i32 i32 i32 i32) (result i32))))",
        R"((data (i32.const 0) "abc"))",
        "(call $f (i32.const 0) (i32.const 3) (i32.const 8) (i32.const 32))",
        k);
}

std::string
traceWat(std::uint64_t k)
{
    return makeLoopWat(
        R"((import "host" "trace" (func $f (param i32 i32 i32 i32 i32) (result i32))))",
        R"((data (i32.const 0) "hi"))",
        "(call $f (i32.const 0) (i32.const 2) (i32.const 0) (i32.const 0) (i32.const 0))",
        k);
}

std::string
traceNumWat(std::uint64_t k)
{
    return makeLoopWat(
        R"((import "host" "trace_num" (func $f (param i32 i32 i64) (result i32))))",
        R"((data (i32.const 0) "hi"))",
        "(call $f (i32.const 0) (i32.const 2) (i64.const 99))",
        k);
}

using WatGen = std::string (*)(std::uint64_t);

// Drives the C++ engine's real production entry point (`xrpl::runEscrowWasm`)
// once per timed iteration. `code` is pre-assembled and `host` pre-built
// outside the loop, per the measurement model -- only the full
// validate+instantiate+execute "finish" call is timed. One untimed sanity
// call up front turns a broken contract/host wiring into a loud
// `SkipWithError` instead of a silently-bogus (near-zero) timing.
void
runCppBenchmark(benchmark::State& state, Bytes const& code, FakeHost& host)
{
    auto const warm = xrpl::runEscrowWasm(code, host, static_cast<std::int64_t>(GAS));
    if (!warm.has_value())
    {
        state.SkipWithError("C++ engine failed to run the benchmark contract");
        return;
    }
    for (auto _ : state)
    {
        auto r = xrpl::runEscrowWasm(code, host, static_cast<std::int64_t>(GAS));
        benchmark::DoNotOptimize(r);
    }
}

// Drives the Rust engine's real production entry point
// (`xrpl::wasmrs::runEscrowWasmRsWithCxxHost`, the cxx-host path) once per
// timed iteration, mirroring `runCppBenchmark` above. This entry throws
// `rust::Error` (rather than returning an in-band error) on failure -- and
// `rust::Error` derives from `std::exception` -- so the untimed sanity call
// is wrapped the same way `runCppBenchmark`'s is, turning a wiring bug into a
// loud `SkipWithError` instead of a crash or a silently-bogus timing.
void
runRustBenchmark(benchmark::State& state, std::vector<std::uint8_t> const& code, FakeHost& host)
{
    try
    {
        auto const warm = xrpl::wasmrs::runEscrowWasmRsWithCxxHost(code, host, GAS);
        benchmark::DoNotOptimize(warm);
    }
    catch (std::exception const& e)
    {
        state.SkipWithError((std::string("Rust engine failed to run the benchmark contract: ") + e.what()).c_str());
        return;
    }
    for (auto _ : state)
    {
        auto r = xrpl::wasmrs::runEscrowWasmRsWithCxxHost(code, host, GAS);
        benchmark::DoNotOptimize(r);
    }
}

void
BM_Cpp_Launch(benchmark::State& state)
{
    auto const code = assemble(std::string(kNoOpWat));
    FakeHost host;
    runCppBenchmark(state, code, host);
}

void
BM_Rust_Launch(benchmark::State& state)
{
    auto const code = assemble(std::string(kNoOpWat));
    FakeHost host;
    runRustBenchmark(state, code, host);
}

// Registers a C++-engine benchmark swept over K (the per-run host-call loop
// count): `RangeMultiplier(8)->Range(1, 8192)` yields K in {1, 8, 64, 512,
// 4096, 8192}. The transfer-limit note in CLAUDE.md's benchmark task caps K
// at 8192: both engines enforce a 1 MiB PER-RUN cumulative host<->guest byte
// budget, and sha512_half is the most bytes-hungry call here at ~35 B/call,
// so 8192 * 35 ~= 287 KB stays well under the cap. `SetComplexityN(K)` +
// `Complexity(oN)` reports a per-call cost alongside the raw per-run time.
void
registerCppHostFnBenchmark(char const* name, WatGen watGen)
{
    auto* b = benchmark::RegisterBenchmark(name, [watGen](benchmark::State& state) {
        auto const k = static_cast<std::uint64_t>(state.range(0));
        auto const code = assemble(watGen(k));
        FakeHost host;
        runCppBenchmark(state, code, host);
        state.SetComplexityN(static_cast<std::int64_t>(k));
    });
    b->RangeMultiplier(8)->Range(1, 8192)->Complexity(benchmark::oN);
}

// Rust-engine counterpart of `registerCppHostFnBenchmark`; see its comment
// for the K sweep / transfer-limit rationale (identical for both engines).
void
registerRustHostFnBenchmark(char const* name, WatGen watGen)
{
    auto* b = benchmark::RegisterBenchmark(name, [watGen](benchmark::State& state) {
        auto const k = static_cast<std::uint64_t>(state.range(0));
        auto const code = assemble(watGen(k));
        FakeHost host;
        runRustBenchmark(state, code, host);
        state.SetComplexityN(static_cast<std::int64_t>(k));
    });
    b->RangeMultiplier(8)->Range(1, 8192)->Complexity(benchmark::oN);
}

// Registration happens via this namespace-scope initializer (rather than the
// `BENCHMARK(...)` self-registering macro) so the per-host-function
// benchmarks can be generated in a loop-like fashion and chain
// `RangeMultiplier`/`Range`/`Complexity`, while still finishing before
// `BENCHMARK_MAIN`'s `main()` runs `RunSpecifiedBenchmarks()` (guaranteed:
// namespace-scope initializers in this translation unit run before `main`,
// regardless of which TU defines `main`).
[[maybe_unused]] bool const g_registered = [] {
    benchmark::RegisterBenchmark("cpp/launch", BM_Cpp_Launch);
    benchmark::RegisterBenchmark("rust/launch", BM_Rust_Launch);

    registerCppHostFnBenchmark("cpp/ldgr_index", ldgrIndexWat);
    registerRustHostFnBenchmark("rust/ldgr_index", ldgrIndexWat);

    registerCppHostFnBenchmark("cpp/home_le_field", homeLeFieldWat);
    registerRustHostFnBenchmark("rust/home_le_field", homeLeFieldWat);

    registerCppHostFnBenchmark("cpp/sha512_half", sha512HalfWat);
    registerRustHostFnBenchmark("rust/sha512_half", sha512HalfWat);

    registerCppHostFnBenchmark("cpp/trace", traceWat);
    registerRustHostFnBenchmark("rust/trace", traceWat);

    registerCppHostFnBenchmark("cpp/trace_num", traceNumWat);
    registerRustHostFnBenchmark("rust/trace_num", traceNumWat);

    return true;
}();

}  // namespace

BENCHMARK_MAIN();
