#include <xrpl/tx/wasm-rs/WasmVmRs.h>

#include <gtest/gtest.h>

#include <xrpl/basics/Slice.h>
#include <xrpl/protocol/digest.h>

#include <cstdint>
#include <string_view>

using namespace xrpl;

// A wasm module whose `finish` export calls the `ldgr_index` host import and
// returns it (i64 -> i32). The Rust SampleHost answers `ldgr_index` with 7.
static constexpr std::string_view kLedgerSqnWat = R"wat(
    (module
      (import "host" "ldgr_index" (func $ldgr_index (result i64)))
      (func (export "finish") (result i32)
        (i32.wrap_i64 (call $ldgr_index))))
)wat";

TEST(WasmVmRs, runs_escrow_against_mock_host)
{
    auto const r = wasmrs::runEscrowWasmRsFromWat(kLedgerSqnWat, 1'000'000);
    EXPECT_EQ(r.result, 7);      // SampleHost::get_ledger_sqn() == 7
    EXPECT_GT(r.fuelUsed, 0u);   // gas was consumed
}

TEST(WasmVmRs, forwards_sha512_half_to_cxx)
{
    // Guest hashes the 3 bytes "abc" via the sha512_half host fn, writes the
    // 32-byte digest at offset 64, and returns digest byte[0].
    static constexpr std::string_view kWat = R"wat(
        (module
          (import "host" "sha512_half"
            (func $sha (param i32 i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "abc")
          (func (export "finish") (result i32)
            (drop (call $sha (i32.const 0) (i32.const 3) (i32.const 64) (i32.const 32)))
            (i32.load8_u (i32.const 64))))
    )wat";

    auto const r = wasmrs::runEscrowWasmRsFromWatWithCxxHost(kWat, 1'000'000);

    // Independently compute the same digest with the C++ primitive.
    std::uint8_t const abc[] = {'a', 'b', 'c'};
    auto const expected = sha512Half(Slice(abc, 3));
    EXPECT_EQ(r.result, static_cast<std::int32_t>(expected.data()[0]));
    EXPECT_GT(r.fuelUsed, 0u);
}
