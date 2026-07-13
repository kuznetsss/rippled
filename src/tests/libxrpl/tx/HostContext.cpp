#include <xrpl/tx/wasm-rs/HostContext.h>

#include <xrpl/basics/Slice.h>
#include <xrpl/protocol/SField.h>
#include <xrpl/tx/wasm/HostFunc.h>    // xrpl::HostFunctions (complete type)
#include <xrpl/tx/wasm/WasmCommon.h>  // hfErrorToInt

#include <rs_wasm_vm_cxxbridge/ffi.h>  // complete Hash / HashResult / BytesResult

#include <algorithm>
#include <string_view>

namespace xrpl::wasmrs {

rs::wasm_vm::HashResult
HostContext::sha512_half(rust::Slice<std::uint8_t const> data) const
{
    auto const r = hf.computeSha512HalfHash(Slice(data.data(), data.size()));
    if (!r)
    {
        return rs::wasm_vm::HashResult{
            .status = hfErrorToInt(r.error()), .value = rs::wasm_vm::Hash{}};
    }
    rs::wasm_vm::Hash out{};
    std::copy(r->data(), r->data() + r->size(), out.data.begin());
    return rs::wasm_vm::HashResult{.status = 0, .value = out};
}

std::int64_t
HostContext::get_ledger_sqn() const
{
    auto const r = hf.getLedgerSqn();
    if (!r)
        return static_cast<std::int64_t>(hfErrorToInt(r.error()));
    return static_cast<std::int64_t>(*r);
}

rs::wasm_vm::BytesResult
HostContext::get_current_ledger_obj_field(std::int32_t field) const
{
    auto const& m = SField::getKnownCodeToField();
    auto const it = m.find(field);
    if (it == m.end())
    {
        return rs::wasm_vm::BytesResult{
            .status = hfErrorToInt(HostFunctionError::InvalidField), .data = {}};
    }

    auto const r = hf.getCurrentLedgerObjField(*it->second);
    if (!r)
        return rs::wasm_vm::BytesResult{.status = hfErrorToInt(r.error()), .data = {}};

    rs::wasm_vm::BytesResult out{.status = 0, .data = {}};
    out.data.reserve(r->size());
    for (auto b : *r)
        out.data.push_back(b);
    return out;
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
