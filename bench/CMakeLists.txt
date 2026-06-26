# SHA-512 ("sha512Half") backend micro-benchmark.
#
# Builds the same source (sha512_bench.cpp) twice: once linked against the
# project's OpenSSL (from Conan) and once against aws-lc, pulled in here via
# FetchContent. Linking both libcryptos into one binary would collide on the
# SHA512_* symbols, so each backend gets its own executable; compare_sha512.sh
# runs both and diffs the numbers.
#
# Enable with -Dbench=ON. aws-lc is built from source, so Go and Perl must be
# on PATH (they generate aws-lc's optimized assembly; without them aws-lc falls
# back to a much slower C-only build and the comparison is meaningless).

set(bench_awslc_tag "v5.0.0" CACHE STRING "aws-lc git tag to benchmark against")

# aws-lc build knobs. Set as normal (non-cache) variables so they are scoped to
# this subdirectory and honored by aws-lc's option() calls (CMP0077 is NEW).
set(BUILD_TESTING OFF)
set(BUILD_TOOL OFF)
set(FIPS OFF)
set(DISABLE_GO OFF)
set(DISABLE_PERL OFF)

FetchContent_Declare(
    awslc
    GIT_REPOSITORY https://github.com/aws/aws-lc.git
    GIT_TAG ${bench_awslc_tag}
    GIT_SHALLOW TRUE
    EXCLUDE_FROM_ALL
)
FetchContent_MakeAvailable(awslc)

# aws-lc exports its libcrypto as the target `crypto`.
if(TARGET crypto)
    set(awslc_crypto crypto)
elseif(TARGET AWS::LC::crypto)
    set(awslc_crypto AWS::LC::crypto)
else()
    message(FATAL_ERROR "aws-lc crypto target not found after FetchContent")
endif()

add_executable(bench_sha512_openssl sha512_bench.cpp)
target_link_libraries(bench_sha512_openssl PRIVATE OpenSSL::Crypto)
target_compile_definitions(bench_sha512_openssl PRIVATE BENCH_BACKEND="OpenSSL")

add_executable(bench_sha512_awslc sha512_bench.cpp)
target_link_libraries(bench_sha512_awslc PRIVATE ${awslc_crypto})
target_compile_definitions(bench_sha512_awslc PRIVATE BENCH_BACKEND="aws-lc")

set_target_properties(
    bench_sha512_openssl
    bench_sha512_awslc
    PROPERTIES RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}"
)
