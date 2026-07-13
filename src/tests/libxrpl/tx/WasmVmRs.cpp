#include <xrpl/tx/wasm-rs/WasmVmRs.h>

#include <gtest/gtest.h>

#include <xrpl/basics/Slice.h>
#include <xrpl/protocol/SField.h>
#include <xrpl/protocol/digest.h>
#include <xrpl/tx/wasm/HostFunc.h>

#include <cstdint>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

using namespace xrpl;

// A wasm module whose `finish` export calls the `ldgr_index` host import and
// returns it (i64 -> i32). Reused against both the SampleHost (Rust-side mock,
// answers 7) and a C++ FakeHost (answers 42), since it's the same guest
// program either way.
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

namespace {

// Minimal C++ mock of the host — the whole point of wrapping the virtual
// base: C++ can hand the engine a test double with zero ledger context.
struct FakeHost : HostFunctions
{
    mutable std::vector<std::pair<std::string, std::int64_t>> tracedNums;

    std::expected<std::uint32_t, HostFunctionError>
    getLedgerSqn() const override
    {
        return 42u;
    }

    std::expected<Hash, HostFunctionError>
    computeSha512HalfHash(Slice const& data) const override
    {
        return sha512Half(data);
    }

    std::expected<Bytes, HostFunctionError>
    getCurrentLedgerObjField(SField const&) const override
    {
        return Bytes{0xAA, 0xBB, 0xCC};
    }

    std::expected<int32_t, HostFunctionError>
    traceNum(std::string_view const& msg, int64_t data) const override
    {
        tracedNums.emplace_back(std::string(msg), data);
        return 0;
    }

    std::expected<int32_t, HostFunctionError>
    trace(std::string_view const&, Slice const&, bool) const override
    {
        return 0;
    }
};

// Builds a wasm module whose `finish` export calls `home_le_field(fieldCode,
// 64, 16)`, dropping the byte count, then returns byte[0] of what got written
// at offset 64.
std::string
makeFieldWat(std::int32_t fieldCode)
{
    return "(module\n"
        "  (import \"host\" \"home_le_field\"\n"
        "    (func $home_le_field (param i32 i32 i32) (result i32)))\n"
        "  (memory (export \"memory\") 1)\n"
        "  (func (export \"finish\") (result i32)\n"
        "    (drop (call $home_le_field (i32.const " +
        std::to_string(fieldCode) +
        ") (i32.const 64) (i32.const 16)))\n"
        "    (i32.load8_u (i32.const 64))))\n";
}

}  // namespace

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

    FakeHost fake;
    auto const r = wasmrs::runEscrowWasmRsFromWatWithCxxHost(kWat, fake, 1'000'000);

    // Independently compute the same digest with the C++ primitive.
    std::uint8_t const abc[] = {'a', 'b', 'c'};
    auto const expected = sha512Half(Slice(abc, 3));
    EXPECT_EQ(r.result, static_cast<std::int32_t>(expected.data()[0]));
    EXPECT_GT(r.fuelUsed, 0u);
}

TEST(WasmVmRs, forwards_get_ledger_sqn_to_cxx)
{
    FakeHost fake;
    auto const r = wasmrs::runEscrowWasmRsFromWatWithCxxHost(kLedgerSqnWat, fake, 1'000'000);
    EXPECT_EQ(r.result, 42);     // FakeHost::getLedgerSqn() == 42
    EXPECT_GT(r.fuelUsed, 0u);
}

TEST(WasmVmRs, forwards_field_getter_to_cxx)
{
    // sfFlags is a real, known field code (SField::getKnownCodeToField()
    // contains it), obtained at runtime rather than hardcoded so the test
    // doesn't depend on the type/index encoding staying stable.
    auto const wat = makeFieldWat(sfFlags.getCode());

    FakeHost fake;
    auto const r = wasmrs::runEscrowWasmRsFromWatWithCxxHost(wat, fake, 1'000'000);
    EXPECT_EQ(r.result, 0xAA);  // FakeHost::getCurrentLedgerObjField()[0] == 0xAA
    EXPECT_GT(r.fuelUsed, 0u);
}

TEST(WasmVmRs, forwards_trace_num_to_cxx)
{
    // Guest calls trace_num("hi", 99) then returns 1.
    static constexpr std::string_view kWat = R"wat(
        (module
          (import "host" "trace_num"
            (func $trace_num (param i32 i32 i64) (result i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "hi")
          (func (export "finish") (result i32)
            (drop (call $trace_num (i32.const 0) (i32.const 2) (i64.const 99)))
            (i32.const 1)))
    )wat";

    FakeHost fake;
    auto const r = wasmrs::runEscrowWasmRsFromWatWithCxxHost(kWat, fake, 1'000'000);
    EXPECT_EQ(r.result, 1);
    ASSERT_EQ(fake.tracedNums.size(), 1u);
    EXPECT_EQ(fake.tracedNums[0].first, "hi");
    EXPECT_EQ(fake.tracedNums[0].second, 99);
}
