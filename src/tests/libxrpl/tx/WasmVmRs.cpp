#include <xrpl/tx/wasm-rs/WasmVmRs.h>

#include <gtest/gtest.h>

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
