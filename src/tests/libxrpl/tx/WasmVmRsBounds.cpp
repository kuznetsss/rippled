#include <xrpl/protocol/SField.h>
#include <xrpl/protocol/digest.h>
#include <xrpl/tx/wasm-rs/WasmVmRs.h>
#include <xrpl/tx/wasm/HostFunc.h>
#include <xrpl/tx/wasm/WasmCommon.h>  // HostFunctionError, hfErrorToInt

#include "xrpl/basics/Slice.h"
#include <gtest/gtest.h>

#include <cstdint>
#include <expected>
#include <string_view>

// Adversarial bounds tests for the Rust `wasm_vm` engine's host-function ABI.
//
// Each test drives a hostile guest contract that hands a host function
// out-of-bounds / negative / oversized pointer+size arguments, attempting to
// make the host read or write outside the guest's own linear memory. Every one
// must be rejected with a negative `HostFunctionError` code and *no* out-of-
// bounds access — the whole point of the checked-slice "unsafe surface" in
// `crates/wasm_vm/src/abi.rs`.
//
// Mechanics: a host function returning a negative status is a *normal* wasm
// return (the marshaling closures return `i32`, they never trap), so a guest
// whose `finish` export returns the host call's status *directly* (rather than
// dropping it) lets the C++ side read that status straight out of
// `EscrowRunResult::result`. Assertions therefore compare `r.result` against
// the expected `HostFunctionError` wire code.
//
// Everything runs through `runEscrowWasmRsFromWatWithCxxHost`, i.e. the full
// Rust -> C++ `HostContext` forwarding path, so both the engine's bounds checks
// and the C++ host shim are exercised together.

using namespace xrpl;

namespace {

// One page of linear memory is declared in every module below.
constexpr std::int32_t kPageBytes = 64 * 1024;  // 65536

// An offset comfortably past the end of a 1-page memory.
constexpr std::int32_t kPastEnd = kPageBytes + 100;

// An offset near the very end, chosen so a small region straddles the boundary.
constexpr std::int32_t kStraddleEnd = kPageBytes - 2;

// A length larger than any allowed field, and far past the memory size, used to
// probe the "absurd size" paths.
constexpr std::int32_t kHugeLen = 2'000'000'000;

// Wire codes, taken straight from the shared C++ enum so the tests can't drift
// from the discriminants the Rust engine mirrors.
constexpr std::int32_t kInvalidParams = hfErrorToInt(HostFunctionError::InvalidParams);
constexpr std::int32_t kPointerOob = hfErrorToInt(HostFunctionError::PointerOutOfBounds);
constexpr std::int32_t kDataTooLarge = hfErrorToInt(HostFunctionError::DataFieldTooLarge);
constexpr std::int32_t kBufTooSmall = hfErrorToInt(HostFunctionError::BufferTooSmall);

constexpr std::uint64_t kGas = 1'000'000;

// A host with deterministic, known-size answers for the five PoC host
// functions, so the buffer-fit ("too small") outcomes are predictable:
//   get_ledger_sqn          -> 4 bytes (serialized u32)
//   sha512_half             -> 32 bytes (digest)
//   getCurrentLedgerObjField-> 3 bytes
//   trace / trace_num       -> ok (0)
struct BoundsFakeHost : HostFunctions
{
    [[nodiscard]] std::expected<std::uint32_t, HostFunctionError>
    getLedgerSqn() const override
    {
        return 42u;
    }

    [[nodiscard]] std::expected<Hash, HostFunctionError>
    computeSha512HalfHash(Slice const& data) const override
    {
        return sha512Half(data);
    }

    [[nodiscard]] std::expected<Bytes, HostFunctionError>
    getCurrentLedgerObjField(SField const&) const override
    {
        return Bytes{0xAA, 0xBB, 0xCC};
    }

    [[nodiscard]] std::expected<int32_t, HostFunctionError>
    traceNum(std::string_view const&, int64_t) const override
    {
        return 0;
    }

    [[nodiscard]] std::expected<int32_t, HostFunctionError>
    trace(std::string_view const&, Slice const&, bool) const override
    {
        return 0;
    }
};

// Run `wat` (whose `finish` returns a host call's status directly) and hand back
// that status. A fresh host per call keeps the cases independent.
std::int32_t
runStatus(std::string const& wat)
{
    BoundsFakeHost fake;
    return wasmrs::runEscrowWasmRsFromWatWithCxxHost(wat, fake, kGas).result;
}

// ---- WAT builders: `finish` returns the host call's status verbatim. --------

// ldgr_index(out_ptr, out_len) -> status
std::string
watLedgerSqn(std::int32_t outPtr, std::int32_t outLen)
{
    return "(module\n"
           "  (import \"host\" \"ldgr_index\"\n"
           "    (func $f (param i32 i32) (result i32)))\n"
           "  (memory (export \"memory\") 1)\n"
           "  (func (export \"finish\") (result i32)\n"
           "    (call $f (i32.const " +
        std::to_string(outPtr) + ") (i32.const " + std::to_string(outLen) + "))))\n";
}

// home_le_field(field, out_ptr, out_len) -> status
std::string
watField(std::int32_t field, std::int32_t outPtr, std::int32_t outLen)
{
    return "(module\n"
           "  (import \"host\" \"home_le_field\"\n"
           "    (func $f (param i32 i32 i32) (result i32)))\n"
           "  (memory (export \"memory\") 1)\n"
           "  (func (export \"finish\") (result i32)\n"
           "    (call $f (i32.const " +
        std::to_string(field) + ") (i32.const " + std::to_string(outPtr) + ") (i32.const " +
        std::to_string(outLen) + "))))\n";
}

// sha512_half(data_ptr, data_len, out_ptr, out_len) -> status
std::string
watSha512(std::int32_t dataPtr, std::int32_t dataLen, std::int32_t outPtr, std::int32_t outLen)
{
    return "(module\n"
           "  (import \"host\" \"sha512_half\"\n"
           "    (func $f (param i32 i32 i32 i32) (result i32)))\n"
           "  (memory (export \"memory\") 1)\n"
           "  (func (export \"finish\") (result i32)\n"
           "    (call $f (i32.const " +
        std::to_string(dataPtr) + ") (i32.const " + std::to_string(dataLen) + ") (i32.const " +
        std::to_string(outPtr) + ") (i32.const " + std::to_string(outLen) + "))))\n";
}

// trace(msg_ptr, msg_len, data_ptr, data_len, as_hex) -> status
std::string
watTrace(std::int32_t msgPtr, std::int32_t msgLen, std::int32_t dataPtr, std::int32_t dataLen)
{
    return "(module\n"
           "  (import \"host\" \"trace\"\n"
           "    (func $f (param i32 i32 i32 i32 i32) (result i32)))\n"
           "  (memory (export \"memory\") 1)\n"
           "  (func (export \"finish\") (result i32)\n"
           "    (call $f (i32.const " +
        std::to_string(msgPtr) + ") (i32.const " + std::to_string(msgLen) + ") (i32.const " +
        std::to_string(dataPtr) + ") (i32.const " + std::to_string(dataLen) +
        ") (i32.const 0))))\n";
}

// trace_num(msg_ptr, msg_len, number) -> status
std::string
watTraceNum(std::int32_t msgPtr, std::int32_t msgLen)
{
    return "(module\n"
           "  (import \"host\" \"trace_num\"\n"
           "    (func $f (param i32 i32 i64) (result i32)))\n"
           "  (memory (export \"memory\") 1)\n"
           "  (func (export \"finish\") (result i32)\n"
           "    (call $f (i32.const " +
        std::to_string(msgPtr) + ") (i32.const " + std::to_string(msgLen) + ") (i64.const 99))))\n";
}

// A known-good, in-bounds field code used by the field-getter tests.
std::int32_t
validFieldCode()
{
    return static_cast<std::int32_t>(sfFlags.getCode());
}

}  // namespace

// ---------------------------------------------------------------------------
// Output-only host functions (write_into): the guest points the *write* target
// out of bounds / negative / at a too-small buffer.
// ---------------------------------------------------------------------------

TEST(WasmVmRsBounds, ldgr_index_output_escapes)
{
    struct Case
    {
        std::int32_t outPtr, outLen, expect;
        char const* name;
    };
    // get_ledger_sqn writes 4 bytes.
    Case const cases[] = {
        {.outPtr = -1, .outLen = 4, .expect = kInvalidParams, .name = "negative out ptr"},
        {.outPtr = 0, .outLen = -1, .expect = kInvalidParams, .name = "negative out len"},
        {.outPtr = kPastEnd, .outLen = 4, .expect = kPointerOob, .name = "out ptr past memory end"},
        {.outPtr = kStraddleEnd,
         .outLen = 4,
         .expect = kPointerOob,
         .name = "out region straddles memory end"},
        {.outPtr = 0,
         .outLen = kHugeLen,
         .expect = kPointerOob,
         .name = "absurd out len overruns memory"},
        {.outPtr = 0,
         .outLen = 2,
         .expect = kBufTooSmall,
         .name = "buffer smaller than 4-byte value"},
    };
    for (auto const& c : cases)
    {
        SCOPED_TRACE(c.name);
        EXPECT_EQ(runStatus(watLedgerSqn(c.outPtr, c.outLen)), c.expect);
    }
}

TEST(WasmVmRsBounds, home_le_field_output_escapes)
{
    auto const field = validFieldCode();  // value present -> 3 bytes
    struct Case
    {
        std::int32_t outPtr, outLen, expect;
        char const* name;
    };
    Case const cases[] = {
        {.outPtr = -1, .outLen = 8, .expect = kInvalidParams, .name = "negative out ptr"},
        {.outPtr = 64, .outLen = -1, .expect = kInvalidParams, .name = "negative out len"},
        {.outPtr = kPastEnd, .outLen = 8, .expect = kPointerOob, .name = "out ptr past memory end"},
        {.outPtr = kStraddleEnd,
         .outLen = 8,
         .expect = kPointerOob,
         .name = "out region straddles memory end"},
        {.outPtr = 0,
         .outLen = kHugeLen,
         .expect = kPointerOob,
         .name = "absurd out len overruns memory"},
        {.outPtr = 64,
         .outLen = 2,
         .expect = kBufTooSmall,
         .name = "buffer smaller than 3-byte value"},
    };
    for (auto const& c : cases)
    {
        SCOPED_TRACE(c.name);
        EXPECT_EQ(runStatus(watField(field, c.outPtr, c.outLen)), c.expect);
    }
}

// ---------------------------------------------------------------------------
// sha512_half (read_write): the guest can escape on either the *input* read
// region or the *output* write region. Each group holds the other region valid
// so the failure is attributable to the region under test.
// ---------------------------------------------------------------------------

TEST(WasmVmRsBounds, sha512_half_input_escapes)
{
    // Output region kept valid and roomy: [4096, 4096+32).
    constexpr std::int32_t outPtr = 4096, outLen = 32;
    struct Case
    {
        std::int32_t dataPtr, dataLen, expect;
        char const* name;
    };
    Case const cases[] = {
        {.dataPtr = -1, .dataLen = 8, .expect = kInvalidParams, .name = "negative data ptr"},
        {.dataPtr = 0, .dataLen = -1, .expect = kInvalidParams, .name = "negative data len"},
        {.dataPtr = 0,
         .dataLen = 2000,
         .expect = kDataTooLarge,
         .name = "data len over 1KiB field cap"},
        {.dataPtr = 0,
         .dataLen = kHugeLen,
         .expect = kDataTooLarge,
         .name = "absurd data len hits field cap, not bounds"},
        {.dataPtr = kPastEnd,
         .dataLen = 8,
         .expect = kPointerOob,
         .name = "data ptr past memory end"},
        {.dataPtr = kStraddleEnd,
         .dataLen = 8,
         .expect = kPointerOob,
         .name = "data region straddles memory end"},
    };
    for (auto const& c : cases)
    {
        SCOPED_TRACE(c.name);
        EXPECT_EQ(runStatus(watSha512(c.dataPtr, c.dataLen, outPtr, outLen)), c.expect);
    }
}

TEST(WasmVmRsBounds, sha512_half_output_escapes)
{
    // Input region kept valid: 3 in-bounds bytes (zero-initialized memory).
    constexpr std::int32_t dataPtr = 0, dataLen = 3;
    struct Case
    {
        std::int32_t outPtr, outLen, expect;
        char const* name;
    };
    // Digest is 32 bytes.
    Case const cases[] = {
        {.outPtr = -1, .outLen = 32, .expect = kInvalidParams, .name = "negative out ptr"},
        {.outPtr = 4096, .outLen = -1, .expect = kInvalidParams, .name = "negative out len"},
        {.outPtr = kPastEnd,
         .outLen = 32,
         .expect = kPointerOob,
         .name = "out ptr past memory end"},
        {.outPtr = kStraddleEnd,
         .outLen = 32,
         .expect = kPointerOob,
         .name = "out region straddles memory end"},
        {.outPtr = 4096,
         .outLen = 16,
         .expect = kBufTooSmall,
         .name = "buffer smaller than 32-byte digest"},
    };
    for (auto const& c : cases)
    {
        SCOPED_TRACE(c.name);
        EXPECT_EQ(runStatus(watSha512(dataPtr, dataLen, c.outPtr, c.outLen)), c.expect);
    }
}

// ---------------------------------------------------------------------------
// Read-only host functions (read_borrowed): trace has two read regions (msg,
// data); trace_num has one (msg). The host must never read outside guest
// memory.
// ---------------------------------------------------------------------------

TEST(WasmVmRsBounds, trace_msg_escapes)
{
    // data region kept valid (empty).
    constexpr std::int32_t dataPtr = 0, dataLen = 0;
    struct Case
    {
        std::int32_t msgPtr, msgLen, expect;
        char const* name;
    };
    Case const cases[] = {
        {.msgPtr = -1, .msgLen = 4, .expect = kInvalidParams, .name = "negative msg ptr"},
        {.msgPtr = 0, .msgLen = -1, .expect = kInvalidParams, .name = "negative msg len"},
        {.msgPtr = 0,
         .msgLen = 2000,
         .expect = kDataTooLarge,
         .name = "msg len over 1KiB field cap"},
        {.msgPtr = kPastEnd, .msgLen = 4, .expect = kPointerOob, .name = "msg ptr past memory end"},
        {.msgPtr = kStraddleEnd,
         .msgLen = 4,
         .expect = kPointerOob,
         .name = "msg region straddles memory end"},
    };
    for (auto const& c : cases)
    {
        SCOPED_TRACE(c.name);
        EXPECT_EQ(runStatus(watTrace(c.msgPtr, c.msgLen, dataPtr, dataLen)), c.expect);
    }
}

TEST(WasmVmRsBounds, trace_data_escapes)
{
    // msg region kept valid (empty -> valid UTF-8).
    constexpr std::int32_t msgPtr = 0, msgLen = 0;
    struct Case
    {
        std::int32_t dataPtr, dataLen, expect;
        char const* name;
    };
    Case const cases[] = {
        {.dataPtr = -1, .dataLen = 4, .expect = kInvalidParams, .name = "negative data ptr"},
        {.dataPtr = 0, .dataLen = -1, .expect = kInvalidParams, .name = "negative data len"},
        {.dataPtr = 0,
         .dataLen = 2000,
         .expect = kDataTooLarge,
         .name = "data len over 1KiB field cap"},
        {.dataPtr = kPastEnd,
         .dataLen = 4,
         .expect = kPointerOob,
         .name = "data ptr past memory end"},
        {.dataPtr = kStraddleEnd,
         .dataLen = 4,
         .expect = kPointerOob,
         .name = "data region straddles memory end"},
    };
    for (auto const& c : cases)
    {
        SCOPED_TRACE(c.name);
        EXPECT_EQ(runStatus(watTrace(msgPtr, msgLen, c.dataPtr, c.dataLen)), c.expect);
    }
}

TEST(WasmVmRsBounds, trace_num_msg_escapes)
{
    struct Case
    {
        std::int32_t msgPtr, msgLen, expect;
        char const* name;
    };
    Case const cases[] = {
        {.msgPtr = -1, .msgLen = 4, .expect = kInvalidParams, .name = "negative msg ptr"},
        {.msgPtr = 0, .msgLen = -1, .expect = kInvalidParams, .name = "negative msg len"},
        {.msgPtr = 0,
         .msgLen = 2000,
         .expect = kDataTooLarge,
         .name = "msg len over 1KiB field cap"},
        {.msgPtr = kPastEnd, .msgLen = 4, .expect = kPointerOob, .name = "msg ptr past memory end"},
        {.msgPtr = kStraddleEnd,
         .msgLen = 4,
         .expect = kPointerOob,
         .name = "msg region straddles memory end"},
    };
    for (auto const& c : cases)
    {
        SCOPED_TRACE(c.name);
        EXPECT_EQ(runStatus(watTraceNum(c.msgPtr, c.msgLen)), c.expect);
    }
}

// ---------------------------------------------------------------------------
// Control: the same builders with fully in-bounds arguments succeed, returning
// the value's byte count (>= 0). Proves the negative results above come from
// the bounds checks, not from a malformed guest module.
// ---------------------------------------------------------------------------

TEST(WasmVmRsBounds, valid_calls_succeed)
{
    EXPECT_EQ(runStatus(watLedgerSqn(0, 4)), 4);                  // 4-byte sqn
    EXPECT_EQ(runStatus(watField(validFieldCode(), 64, 16)), 3);  // 3-byte field
    EXPECT_EQ(runStatus(watSha512(0, 3, 4096, 32)), 32);          // 32-byte digest
    EXPECT_EQ(runStatus(watTrace(0, 0, 0, 0)), 0);                // trace ok
    EXPECT_EQ(runStatus(watTraceNum(0, 0)), 0);                   // trace_num ok
}
