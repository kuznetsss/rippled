# cmake/XrplRustStdDedup.cmake
#
# Building both wasm engines into one binary links two INDEPENDENT Rust
# staticlibs -- conan's wasmi (the C++ engine's interpreter) and our corrosion
# crates (libwasm_vm.a / librs_hello_world.a) -- and each bundles its own copy
# of the Rust std runtime, double-defining identical runtime symbols
# (`_rust_eh_personality`, `std::panicking::EMPTY_PANIC`, ...).
#
#   * GNU ld / ELF lld (Linux): tolerate identical duplicates with
#     --allow-multiple-definition (keep one; zero codegen impact).
#   * Apple ld64 (macOS): rejects duplicate strong symbols and has no such flag
#     (nor do ld64.lld / mold). So we mark OUR copies weak with llvm-objcopy
#     before the link; ld64 then keeps conan's strong copies. Both copies are
#     byte-identical std, so which survives is irrelevant, and only link-time
#     symbol resolution changes -- no hot-path codegen, so benchmarks stay fair.
#
# macOS Debug happens to link without this (codegen-unit layout), so we only
# weaken in optimized configs to avoid forcing an llvm-objcopy dependency on
# ordinary Debug builds. Linux needs the flag in every configuration.

if(APPLE)
  find_program(LLVM_OBJCOPY NAMES llvm-objcopy
    HINTS
      /opt/homebrew/opt/llvm/bin
      /usr/local/opt/llvm/bin
    DOC "llvm-objcopy, used to weaken duplicate Rust std symbols (brew install llvm)")
endif()

# Make <target> link cleanly despite the duplicate Rust std runtime symbols
# contributed by conan wasmi and the corrosion crates it links.
#
#   xrpl_dedupe_rust_std(<target> [CRATES <lib>...])
#
# CRATES = the corrosion staticlib base names the target links
# (default: wasm_vm), e.g. xrpl_dedupe_rust_std(xrpld CRATES wasm_vm rs_hello_world)
function(xrpl_dedupe_rust_std target)
  cmake_parse_arguments(ARG "" "" "CRATES" ${ARGN})
  if(NOT ARG_CRATES)
    set(ARG_CRATES wasm_vm)
  endif()

  if(NOT APPLE)
    # Linux/ELF: one flag covers all duplicate archives, in every config.
    target_link_options(${target} PRIVATE "LINKER:--allow-multiple-definition")
    return()
  endif()

  # macOS: only needed for optimized builds (Debug links as-is).
  if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    return()
  endif()

  if(NOT LLVM_OBJCOPY)
    message(FATAL_ERROR
      "xrpl_dedupe_rust_std(${target}): llvm-objcopy not found. An optimized "
      "macOS build links two Rust staticlibs whose std copies collide; "
      "llvm-objcopy is needed to weaken the duplicates. Install LLVM "
      "('brew install llvm') or configure with -DLLVM_OBJCOPY=/path/to/llvm-objcopy.")
  endif()

  foreach(crate IN LISTS ARG_CRATES)
    add_custom_command(TARGET ${target} PRE_LINK
      COMMAND ${LLVM_OBJCOPY} -w
              --weaken-symbol=_rust_eh_personality
              --weaken-symbol=*EMPTY_PANIC
              "${CMAKE_BINARY_DIR}/crates/lib${crate}.a"
      COMMENT "Weakening duplicate Rust std symbols in lib${crate}.a (for ${target})"
      VERBATIM)
  endforeach()
endfunction()
