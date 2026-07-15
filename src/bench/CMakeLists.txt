# Head-to-head Google Benchmark harness comparing the C++ wasm engine
# (wasmi-C-API) against the Rust wasm_vm PoC engine (reached over cxx). See
# wasm/WasmEngineBench.cpp and CLAUDE.md's "WASM-VM Rust redesign PoC"
# section. Gated behind the `benchmarks` option (default OFF); requires
# google benchmark from Conan (see conanfile.py).
add_executable(xrpl_wasm_bench wasm/WasmEngineBench.cpp)
patch_nix_binary(xrpl_wasm_bench)
set_target_properties(
    xrpl_wasm_bench
    PROPERTIES RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}"
)
# xrpl.libxrpl transitively pulls in the C++ wasm engine, the
# rs_wasm_vm_cxxbridge target (and its generated include dir, so
# `<rs_wasm_vm_cxxbridge/ffi.h>` and `<rust/cxx.h>` resolve -- see
# src/tests/libxrpl/CMakeLists.txt's identical comment for tx/WasmVmRs.cpp),
# Boost, and the include dirs `WasmVmRs.h`/`HostContext.h` need. No explicit
# link to rs_wasm_vm_cxxbridge or extra include dirs are required here.
target_link_libraries(xrpl_wasm_bench PRIVATE benchmark::benchmark xrpl.libxrpl)
xrpl_dedupe_rust_std(xrpl_wasm_bench)
