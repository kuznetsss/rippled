#include <xrpl/tx/wasm-rs/HostContext.h>

#include <rs_wasm_vm_cxxbridge/ffi.h>  // complete Hash / HashResult

#include <xrpl/basics/Slice.h>
#include <xrpl/protocol/digest.h>

#include <algorithm>

namespace xrpl::wasmrs {

rs::wasm_vm::HashResult
HostContext::sha512_half(rust::Slice<std::uint8_t const> data) const
{
    auto const h = sha512Half(Slice(data.data(), data.size()));  // the EXISTING primitive
    rs::wasm_vm::Hash out{};
    std::copy(h.data(), h.data() + h.size(), out.data.begin());
    return rs::wasm_vm::HashResult{0, out};
}

}  // namespace xrpl::wasmrs
