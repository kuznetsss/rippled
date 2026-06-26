#!/usr/bin/env bash
#
# Run both sha512 benchmark binaries and print a side-by-side comparison.
#
# Usage: bench/compare_sha512.sh [BUILD_DIR]
#   BUILD_DIR defaults to "build" (where the binaries land via
#   RUNTIME_OUTPUT_DIRECTORY = CMAKE_BINARY_DIR).
set -euo pipefail

build_dir="${1:-build}"
oss="${build_dir}/bench_sha512_openssl"
aws="${build_dir}/bench_sha512_awslc"

for bin in "$oss" "$aws"; do
    if [[ ! -x "$bin" ]]; then
        echo "error: benchmark binary not found: $bin" >&2
        echo "build it first: cmake -Dbench=ON ... && cmake --build . --target bench_sha512_openssl bench_sha512_awslc" >&2
        exit 1
    fi
done

oss_csv="$("$oss" --csv)"
aws_csv="$("$aws" --csv)"

echo "OpenSSL : $(printf '%s\n' "$oss_csv" | head -1 | cut -d, -f2)"
echo "aws-lc  : $(printf '%s\n' "$aws_csv" | head -1 | cut -d, -f2)"
echo
echo "ratio = aws-lc ns/hash / OpenSSL ns/hash  (<1.00 means aws-lc is faster)"
echo

awk -F, '
    FNR==NR { oss_ns[$3]=$5; oss_mb[$3]=$7; order[++n]=$3; next }
    { aws_ns[$3]=$5; aws_mb[$3]=$7 }
    END {
        printf "%-20s %12s %12s %9s %12s %12s\n", \
            "workload", "oss ns/h", "aws ns/h", "ratio", "oss MB/s", "aws MB/s"
        for (i = 1; i <= n; i++) {
            w = order[i]
            ratio = aws_ns[w] / oss_ns[w]
            printf "%-20s %12.1f %12.1f %8.2fx %12.0f %12.0f\n", \
                w, oss_ns[w], aws_ns[w], ratio, oss_mb[w], aws_mb[w]
        }
    }
' <(printf '%s\n' "$oss_csv") <(printf '%s\n' "$aws_csv")
