# SHA-512 (`sha512Half`) backend benchmark

This benchmark compares the project's OpenSSL against aws-lc as the libcrypto backend for `sha512Half` — the first 256 bits of a SHA-512 digest. `sha512Half` is the dominant hash in rippled, used everywhere from SHAMap node hashing to transaction ids to account keys. Because both libraries export the same `SHA512_*` symbol names, linking them both into one binary would collide; instead the same source (`sha512_bench.cpp`) is compiled twice, producing two separate executables that can be compared side by side.

## Workloads

Each workload mirrors a real call site in rippled:

- `inner_node_full` — SHAMap inner node: 4-byte prefix + 16×32-byte child hashes (17 Update calls).
- `inner_node_sparse` — inner node with only 2–6 populated children.
- `tx_meta_leaf` — tx+meta leaf: prefix + 150–700B blob + 32B key.
- `tx_id` — transaction id: prefix + 120–400B body.
- `acct_key` — single 32-byte key hash.
- `big_buffer_100k` — one 100 KB buffer (matches the original legacy test; raw streaming throughput; not part of the mix).
- `ledger_mix` — weighted blend (inner_node_full 50%, inner_node_sparse 20%, tx_meta_leaf 15%, tx_id 10%, acct_key 5%) approximating a ledger close.

## Requirements

Go and Perl must be on `PATH`. aws-lc uses them to generate its optimized assembly; without them aws-lc falls back to a much slower C-only build and the comparison is meaningless. Build in Release for meaningful numbers.

## Build & run

```bash
# from the repo root, with a Conan-generated build/ already set up:
cd build
cmake -DCMAKE_TOOLCHAIN_FILE:FILEPATH=build/generators/conan_toolchain.cmake \
      -DCMAKE_BUILD_TYPE=Release -Dbench=ON ..
cmake --build . --target bench_sha512_openssl bench_sha512_awslc --parallel

# from the repo root:
./bench/compare_sha512.sh build
```

You can also run a single binary directly: `./build/bench_sha512_openssl` prints a human-readable table, and `./build/bench_sha512_openssl --csv` prints CSV output.

## Pinning aws-lc

Pass `-Dbench_awslc_tag=v1.73.0` (or any valid git tag) to override the aws-lc version fetched by FetchContent. The default is `v5.0.0`.
