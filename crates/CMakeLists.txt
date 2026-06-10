set(CORROSION_VERSION 0.6.1)

find_package(Corrosion ${CORROSION_VERSION} QUIET)
if(NOT Corrosion_FOUND)
    include(FetchContent)
    FetchContent_Declare(
        Corrosion
        GIT_REPOSITORY https://github.com/corrosion-rs/corrosion.git
        GIT_TAG v${CORROSION_VERSION}
    )
    FetchContent_MakeAvailable(Corrosion)
endif()

corrosion_import_crate(MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml)

# Umbrella target that aggregates all crate-generated code (cxxbridge headers,
# etc.). Build this before running clang-tidy so generated headers are present.
add_custom_target(xrpl_crates)

# Interface library that exposes all crate cxxbridge targets to consumers. Link
# against this instead of individual <name>_cxxbridge targets.
add_library(xrpl_crates_cxxbridge INTERFACE)

# add_xrpl_crate(<name> CRATE <crate> FILES <file>...) Creates a cxxbridge
# target <name>_cxxbridge and registers it with xrpl_crates and
# xrpl_crates_cxxbridge.
function(add_xrpl_crate name)
    cmake_parse_arguments(ARG "" "CRATE" "FILES" ${ARGN})
    corrosion_add_cxxbridge(${name}_cxxbridge CRATE ${ARG_CRATE} FILES
                            ${ARG_FILES}
    )
    # The generated bridge source may `#include` project headers referenced via
    # cxx `include!()` (e.g. extern "C++" callback declarations). Let the
    # codegen target see the public include root so those resolve.
    target_include_directories(
        ${name}_cxxbridge PRIVATE ${CMAKE_SOURCE_DIR}/include
    )
    # Generated cxxbridge headers don't exist at configure time; CMake 3.28+
    # validates INTERFACE_SOURCES on consuming targets. Clear it to skip the
    # existence check — build-time ordering is enforced by the custom commands.
    set_target_properties(${name}_cxxbridge PROPERTIES INTERFACE_SOURCES "")
    add_dependencies(xrpl_crates ${name}_cxxbridge)
    target_link_libraries(xrpl_crates_cxxbridge INTERFACE ${name}_cxxbridge)
endfunction()

add_xrpl_crate(rs_http_client CRATE http_client FILES ffi.rs)
