#include <xrpl/tx/wasm-rs/HostContext.h>

#include <xrpl/basics/Slice.h>
#include <xrpl/protocol/SField.h>
#include <xrpl/tx/wasm/HostFunc.h>    // xrpl::HostFunctions (complete type)
#include <xrpl/tx/wasm/WasmCommon.h>  // hfErrorToInt

#include <cstring>
#include <string_view>

namespace xrpl::wasmrs {

std::int32_t
HostContext::sha512_half(rust::Slice<std::uint8_t const> data, rust::Slice<std::uint8_t> out) const
{
    auto const r = hf.computeSha512HalfHash(Slice(data.data(), data.size()));
    if (!r)
        return hfErrorToInt(r.error());

    // Single copy: straight from the digest into `out`, the slice the engine
    // handed us aliasing guest linear memory. Write only when it fits; the
    // engine enforces the buffer-fit / field-cap / transfer policy from the
    // true length we return.
    auto const n = r->size();
    if (n <= out.size())
        std::memcpy(out.data(), r->data(), n);
    return static_cast<std::int32_t>(n);
}

std::int32_t
HostContext::get_ledger_sqn(rust::Slice<std::uint8_t> out) const
{
    auto const r = hf.getLedgerSqn();
    if (!r)
        return hfErrorToInt(r.error());

    // Serialize the sequence number as 4 little-endian bytes (matching the
    // guest's `u32::from_le_bytes`) straight into `out`.
    std::uint32_t const v = *r;
    std::uint8_t const le[4] = {
        static_cast<std::uint8_t>(v),
        static_cast<std::uint8_t>(v >> 8),
        static_cast<std::uint8_t>(v >> 16),
        static_cast<std::uint8_t>(v >> 24),
    };
    if (sizeof(le) <= out.size())
        std::memcpy(out.data(), le, sizeof(le));
    return static_cast<std::int32_t>(sizeof(le));
}

std::int32_t
HostContext::get_current_ledger_obj_field(std::int32_t field, rust::Slice<std::uint8_t> out) const
{
    auto const& m = SField::getKnownCodeToField();
    auto const it = m.find(field);
    if (it == m.end())
        return hfErrorToInt(HostFunctionError::InvalidField);

    auto const r = hf.getCurrentLedgerObjField(*it->second);
    if (!r)
        return hfErrorToInt(r.error());

    // Single copy: straight from the primitive's buffer into `out`, the slice
    // the engine handed us aliasing guest linear memory — no intermediate.
    // Write only when it fits; the engine enforces the field-size cap,
    // buffer-fit and transfer budget from the true length we return.
    auto const n = r->size();
    if (n <= out.size())
        std::memcpy(out.data(), r->data(), n);
    return static_cast<std::int32_t>(n);
}

std::int32_t
HostContext::trace(rust::Str msg, rust::Slice<std::uint8_t const> data, bool asHex) const
{
    auto const r =
        hf.trace(std::string_view(msg.data(), msg.size()), Slice(data.data(), data.size()), asHex);
    if (!r)
        return hfErrorToInt(r.error());
    return *r;
}

std::int32_t
HostContext::trace_num(rust::Str msg, std::int64_t number) const
{
    auto const r = hf.traceNum(std::string_view(msg.data(), msg.size()), number);
    if (!r)
        return hfErrorToInt(r.error());
    return *r;
}

}  // namespace xrpl::wasmrs
