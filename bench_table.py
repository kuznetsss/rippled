#!/usr/bin/env python3
"""Convert Google Benchmark JSON into a Markdown comparison table.

Reads a benchmark JSON (as emitted by --benchmark_out=... --benchmark_format=json)
and prints:

    | test name | C++ | Rust | Rust/C++ |

where each impl cell is `median ± stddev` in a common time unit.

Benchmarks are discovered, not hardcoded: any run whose name is `cpp/<test>` is
paired with `rust/<test>`. Add more prefixes to IMPLS below if needed.
"""

import json
import sys

# Prefix in the JSON run_name -> column header. Order defines column order.
IMPLS = [("cpp", "C++"), ("rust", "Rust")]

# Which timing to report: "real_time" or "cpu_time".
TIME_KEY = "real_time"


def load(path):
    with open(path) as f:
        return json.load(f)


def collect(benchmarks):
    """Return {test_name: {impl: {"median": x, "stddev": y}}}, order preserved."""
    stats = {}
    order = []
    for b in benchmarks:
        if b.get("run_type") != "aggregate":
            continue
        agg = b.get("aggregate_name")
        if agg not in ("median", "stddev"):
            continue
        run_name = b.get("run_name", "")
        impl, _, test = run_name.partition("/")
        if not test or impl not in dict(IMPLS):
            continue
        if test not in stats:
            stats[test] = {}
            order.append(test)
        stats[test].setdefault(impl, {})[agg] = b[TIME_KEY]
    return stats, order


def pick_unit(ns):
    for scale, unit in ((1e9, "s"), (1e6, "ms"), (1e3, "us"), (1.0, "ns")):
        if ns >= scale:
            return scale, unit
    return 1.0, "ns"


def fmt_cell(entry, scale):
    if not entry or "median" not in entry:
        return "—"
    med = entry["median"] / scale
    std = entry.get("stddev", 0.0) / scale
    return f"{med:.3g} ± {std:.2g}"


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "build/bench_result.json"
    data = load(path)
    stats, order = collect(data.get("benchmarks", []))

    # Pick one unit for the whole table from the median of all median values.
    medians = sorted(
        e["median"]
        for t in stats.values()
        for e in t.values()
        if "median" in e
    )
    if not medians:
        print("No paired benchmarks found.", file=sys.stderr)
        sys.exit(1)
    scale, unit = pick_unit(medians[len(medians) // 2])

    headers = (
        ["test name"] + [f"{name}, {unit}" for _, name in IMPLS] + ["C++/Rust"]
    )
    rows = []
    for test in order:
        cells = [test]
        for prefix, _ in IMPLS:
            cells.append(fmt_cell(stats[test].get(prefix), scale))
        cpp = stats[test].get("cpp", {}).get("median")
        rust = stats[test].get("rust", {}).get("median")
        # C++/Rust: >1 means Rust is faster.
        ratio = f"{cpp / rust:.2f}x" if cpp and rust else "—"
        cells.append(ratio)
        rows.append(cells)

    widths = [
        max(len(headers[i]), *(len(r[i]) for r in rows)) for i in range(len(headers))
    ]

    def line(cells):
        return "| " + " | ".join(c.ljust(widths[i]) for i, c in enumerate(cells)) + " |"

    print(line(headers))
    print("|" + "|".join("-" * (w + 2) for w in widths) + "|")
    for r in rows:
        print(line(r))


if __name__ == "__main__":
    main()
