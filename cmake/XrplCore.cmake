#[===================================================================[
  Exported targets.
#]===================================================================]

include(target_protobuf_sources)

# Protocol buffers cannot participate in a unity build,
# because all the generated sources
# define a bunch of `static const` variables with the same names,
# so we just build them as a separate library.
add_library(xrpl.libpb)
set_target_properties(xrpl.libpb PROPERTIES UNITY_BUILD OFF)
target_protobuf_sources(
    xrpl.libpb
    xrpl/proto
    LANGUAGE cpp
    IMPORT_DIRS include/xrpl/proto
    PROTOS include/xrpl/proto/xrpl.proto
)

file(GLOB_RECURSE protos "include/xrpl/proto/org/*.proto")
target_protobuf_sources(
    xrpl.libpb
    xrpl/proto
    LANGUAGE cpp
    IMPORT_DIRS include/xrpl/proto
    PROTOS "${protos}"
)
target_protobuf_sources(
    xrpl.libpb
    xrpl/proto
    LANGUAGE grpc
    IMPORT_DIRS include/xrpl/proto
    PROTOS "${protos}"
    PLUGIN protoc-gen-grpc=$<TARGET_FILE:gRPC::grpc_cpp_plugin>
    GENERATE_EXTENSIONS .grpc.pb.h .grpc.pb.cc
)

target_compile_options(
    xrpl.libpb
    PUBLIC
        $<$<BOOL:${is_msvc}>:-wd4996>
        $<$<BOOL:${is_xcode}>:
        --system-header-prefix="google/protobuf"
        -Wno-deprecated-dynamic-exception-spec
        >
    PRIVATE
        $<$<BOOL:${is_msvc}>:-wd4065>
        $<$<NOT:$<BOOL:${is_msvc}>>:-Wno-deprecated-declarations>
)

target_link_libraries(xrpl.libpb PUBLIC protobuf::libprotobuf gRPC::grpc++)

# TODO: Clean up the number of library targets later.
add_library(xrpl.imports.main INTERFACE)

target_link_libraries(
    xrpl.imports.main
    INTERFACE
        absl::random_random
        date::date
        ed25519::ed25519
        LibArchive::LibArchive
        OpenSSL::Crypto
        Xrpl::boost
        Xrpl::libs
        Xrpl::opts
        Xrpl::syslibs
        secp256k1::secp256k1
        wasmi::wasmi
        xrpl.libpb
        xxHash::xxhash
        $<$<BOOL:${voidstar}>:antithesis-sdk-cpp>
)

include(add_module)
include(target_link_modules)

# Level 01
add_module(xrpl beast)
target_link_libraries(xrpl.libxrpl.beast PUBLIC xrpl.imports.main)

include(GitInfo)
add_module(xrpl git)
target_compile_definitions(
    xrpl.libxrpl.git
    PRIVATE
        GIT_COMMIT_HASH="${GIT_COMMIT_HASH}"
        GIT_BUILD_BRANCH="${GIT_BUILD_BRANCH}"
)
target_link_libraries(xrpl.libxrpl.git PUBLIC xrpl.imports.main)

# Level 02
add_module(xrpl basics)
target_link_libraries(xrpl.libxrpl.basics PUBLIC xrpl.libxrpl.beast)

# Level 03
add_module(xrpl config)
target_link_libraries(xrpl.libxrpl.config PUBLIC xrpl.libxrpl.basics)

add_module(xrpl json)
target_link_libraries(xrpl.libxrpl.json PUBLIC xrpl.libxrpl.basics)

add_module(xrpl crypto)
target_link_libraries(xrpl.libxrpl.crypto PUBLIC xrpl.libxrpl.basics)

# Level 04
add_module(xrpl protocol)
target_link_libraries(
    xrpl.libxrpl.protocol
    PUBLIC xrpl.libxrpl.crypto xrpl.libxrpl.git xrpl.libxrpl.json
)

# Level 05
add_module(xrpl protocol_autogen)
target_link_libraries(
    xrpl.libxrpl.protocol_autogen
    PUBLIC xrpl.libxrpl.protocol
)

# Level 06
add_module(xrpl core)
target_link_libraries(
    xrpl.libxrpl.core
    PUBLIC
        xrpl.libxrpl.basics
        xrpl.libxrpl.config
        xrpl.libxrpl.json
        xrpl.libxrpl.protocol
        xrpl.libxrpl.protocol_autogen
)

# Level 07
add_module(xrpl resource)
target_link_libraries(xrpl.libxrpl.resource PUBLIC xrpl.libxrpl.protocol)

# Level 08
add_module(xrpl net)
target_link_libraries(
    xrpl.libxrpl.net
    PUBLIC
        xrpl.libxrpl.basics
        xrpl.libxrpl.json
        xrpl.libxrpl.protocol
        xrpl.libxrpl.resource
)

add_module(xrpl nodestore)
target_link_libraries(
    xrpl.libxrpl.nodestore
    PUBLIC
        xrpl.libxrpl.basics
        xrpl.libxrpl.config
        xrpl.libxrpl.json
        xrpl.libxrpl.protocol
)

add_module(xrpl shamap)
target_link_libraries(
    xrpl.libxrpl.shamap
    PUBLIC
        xrpl.libxrpl.basics
        xrpl.libxrpl.crypto
        xrpl.libxrpl.protocol
        xrpl.libxrpl.nodestore
)

add_module(xrpl rdb)
target_link_libraries(
    xrpl.libxrpl.rdb
    PUBLIC xrpl.libxrpl.basics xrpl.libxrpl.config xrpl.libxrpl.core
)

add_module(xrpl server)
target_link_libraries(
    xrpl.libxrpl.server
    PUBLIC
        xrpl.libxrpl.config
        xrpl.libxrpl.protocol
        xrpl.libxrpl.core
        xrpl.libxrpl.rdb
        xrpl.libxrpl.resource
)

add_module(xrpl conditions)
target_link_libraries(xrpl.libxrpl.conditions PUBLIC xrpl.libxrpl.server)

add_module(xrpl ledger)
target_link_libraries(
    xrpl.libxrpl.ledger
    PUBLIC
        xrpl.libxrpl.basics
        xrpl.libxrpl.json
        xrpl.libxrpl.protocol
        xrpl.libxrpl.protocol_autogen
        xrpl.libxrpl.rdb
        xrpl.libxrpl.server
        xrpl.libxrpl.shamap
        xrpl.libxrpl.conditions
)

add_module(xrpl tx)
target_link_libraries(xrpl.libxrpl.tx PUBLIC xrpl.libxrpl.ledger)
# The wasm-rs bridge shim (src/libxrpl/tx/wasm-rs/HostContext.cpp, compiled into
# this tx module) forwards the Rust engine's host calls back to
# xrpl::HostFunctions, and it #includes the generated cxxbridge headers
# (rs_wasm_vm_cxxbridge/ffi.h, rust/cxx.h). So tx needs those headers on its
# include path and must build after they are generated -- but it must NOT *link*
# the bridge: the bridge's ffi.cpp calls back into xrpl::wasmrs::HostContext,
# whose definition lives in libxrpl, making libxrpl and the bridge mutually
# dependent (closed explicitly below). If tx -- an OBJECT library -- linked the
# bridge, it would sit inside that link cycle, and CMake allows link cycles only
# among STATIC libraries. So give tx just the bridge's usage requirements (its
# include dirs) plus a build-order dependency, and let the aggregate
# xrpl.libxrpl carry the actual link (below).
target_include_directories(
    xrpl.libxrpl.tx
    PRIVATE $<TARGET_PROPERTY:rs_wasm_vm_cxxbridge,INTERFACE_INCLUDE_DIRECTORIES>
)
add_dependencies(xrpl.libxrpl.tx rs_wasm_vm_cxxbridge)

add_library(xrpl.libxrpl)
set_target_properties(xrpl.libxrpl PROPERTIES OUTPUT_NAME xrpl)

add_library(xrpl::libxrpl ALIAS xrpl.libxrpl)

file(
    GLOB_RECURSE sources
    CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/src/libxrpl/*.cpp"
)
target_sources(xrpl.libxrpl PRIVATE ${sources})

target_link_modules(
    xrpl
    PUBLIC
    basics
    beast
    conditions
    config
    core
    crypto
    git
    json
    ledger
    net
    nodestore
    protocol
    protocol_autogen
    rdb
    resource
    server
    shamap
    tx
)

# The wasm-rs bridge and xrpl.libxrpl are mutually dependent STATIC libraries:
# libxrpl exposes the bridge (its tx module includes WasmVmRs.h; consumers reach
# the Rust engine through it), and the bridge's generated ffi.cpp calls back into
# xrpl::wasmrs::HostContext, whose definition (HostContext.o) is archived into
# libxrpl.a via the tx module. Declare BOTH edges so CMake knows it is a cycle.
#
# Why it matters: without the back-edge a consumer's link line is
# `... libxrpl.a ... rs_wasm_vm_cxxbridge.a ...` (libxrpl pulls the bridge, so it
# comes first). Apple's ld64 resolves all archives as one group and links fine,
# but GNU ld scans each archive once, left to right: by the time it reaches the
# bridge and discovers the HostContext refs, libxrpl.a is already behind it and
# HostContext.o is never extracted -> "undefined reference" at link. Declaring
# the cycle makes CMake repeat libxrpl.a after the bridge on the link line, so
# GNU ld can extract HostContext.o on the second pass.
#
# The cycle is between two STATIC libraries only (the tx OBJECT library is kept
# out of it -- see the tx include-dir note above), which CMake permits; static
# link cycles impose no build-order cycle, so this is safe. INTERFACE on the
# back-edge: the bridge target itself links nothing, it only needs libxrpl to
# reach the final executable's link line through every bridge consumer.
target_link_libraries(xrpl.libxrpl PUBLIC rs_wasm_vm_cxxbridge)
target_link_libraries(rs_wasm_vm_cxxbridge INTERFACE xrpl.libxrpl)

# All headers in libxrpl are in modules.
# Uncomment this stanza if you have not yet moved new headers into a module.
# target_include_directories(xrpl.libxrpl
#   PRIVATE
#     $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/src>
#   PUBLIC
#     $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
#     $<INSTALL_INTERFACE:include>)

if(xrpld)
    add_executable(xrpld)
    patch_nix_binary(xrpld)
    if(tests)
        target_compile_definitions(xrpld PUBLIC ENABLE_TESTS)
        target_compile_definitions(
            xrpld
            PRIVATE UNIT_TEST_REFERENCE_FEE=${UNIT_TEST_REFERENCE_FEE}
        )
    endif()
    target_include_directories(
        xrpld
        PRIVATE $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/src>
    )

    file(
        GLOB_RECURSE sources
        CONFIGURE_DEPENDS
        "${CMAKE_CURRENT_SOURCE_DIR}/src/xrpld/*.cpp"
    )
    target_sources(xrpld PRIVATE ${sources})

    if(tests)
        file(
            GLOB_RECURSE sources
            CONFIGURE_DEPENDS
            "${CMAKE_CURRENT_SOURCE_DIR}/src/test/*.cpp"
        )
        target_sources(xrpld PRIVATE ${sources})

        target_link_libraries(xrpld rs_hello_world_cxxbridge)
        # rs_hello_world's corrosion staticlib bundles its own Rust std copy,
        # duplicating symbols already pulled in by wasm_vm (see below) --
        # only weaken it here, where it's actually linked.
        xrpl_dedupe_rust_std(xrpld CRATES rs_hello_world)
    endif()

    # rs_wasm_vm_cxxbridge (the Rust wasm-vm engine + the wasm-rs HostContext
    # shim it calls back into) arrives here transitively: xrpl.libxrpl.tx links
    # it PUBLIC, so every consumer of xrpl.libxrpl gets it exactly once. No
    # explicit link needed. It's dead code in the daemon for now (no production
    # path calls the Rust engine yet), but promoting the shim into xrpl.libxrpl
    # is what lets it live in src/ instead of the test tree.
    target_link_libraries(xrpld Xrpl::boost Xrpl::opts Xrpl::libs xrpl.libxrpl)
    # Duplicates conan wasmi's Rust std runtime symbols against our corrosion
    # wasm_vm archive -- see cmake/XrplRustStdDedup.cmake.
    xrpl_dedupe_rust_std(xrpld CRATES wasm_vm)
    exclude_if_included(xrpld)
    # define a macro for tests that might need to
    # be excluded or run differently in CI environment
    if(is_ci)
        target_compile_definitions(xrpld PRIVATE XRPL_RUNNING_IN_CI)
    endif()

    if(voidstar)
        target_compile_options(xrpld PRIVATE -fsanitize-coverage=trace-pc-guard)
        # xrpld requires access to antithesis-sdk-cpp implementation file
        # antithesis_instrumentation.h, which is not exported as INTERFACE
        target_include_directories(
            xrpld
            PRIVATE ${CMAKE_SOURCE_DIR}/external/antithesis-sdk
        )
    endif()

    # The xrpld headers are not built with add_module, so verify them against
    # the executable's own compile environment.
    if(verify_headers)
        verify_target_headers(xrpld "${CMAKE_CURRENT_SOURCE_DIR}/src/xrpld")
        if(tests)
            verify_target_headers(xrpld "${CMAKE_CURRENT_SOURCE_DIR}/src/test")
        endif()
    endif()
endif()
