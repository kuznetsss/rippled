add_executable(http_client_bench main.cpp)
target_compile_features(http_client_bench PRIVATE cxx_std_23)
target_include_directories(http_client_bench PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
find_package(OpenSSL REQUIRED)
find_package(Threads REQUIRED)
target_link_libraries(http_client_bench
    PRIVATE
        xrpl.libxrpl
        Boost::program_options
        OpenSSL::SSL
        OpenSSL::Crypto
        Threads::Threads)
# This is a throwaway benchmark tool — don't run clang-tidy on it.
set_target_properties(http_client_bench PROPERTIES
    CXX_CLANG_TIDY ""
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}")
