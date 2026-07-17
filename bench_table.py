#!/usr/bin/env python3
"""Convert Google Benchmark JSON into Markdown comparison tables.

Reads a benchmark JSON (as emitted by `--benchmark_out=... --benchmark_format=json`,
run with `--benchmark_repetitions>1` so median/stddev aggregates exist) and prints
two tables:

  1. Per-test: the full-run time (median ± stddev, µs) for each engine, plus the
     derived **per-call cost** (ns) and the **per-call speedup**.
  2. Per-host-function summary: the steady-state per-call cost + speedup, one row
     per host-function contract.

Per-call cost = `(run(K) - baseline) / K`, where the **baseline is the same
contract run with K=0 host calls** (`<impl>/<family>/0`): launch + import
resolution + memory, i.e. everything except the host-call loop. Subtracting that
-- rather than the no-import `launch` floor -- is what makes the per-call number
correct: the host-call contracts instantiate a memory and resolve an import that
`launch` does not, so their true fixed cost is several µs higher. Using the wrong
(too-small) baseline leaves that surplus in the numerator, and `surplus / K`
masquerades as a per-call cost that "falls" with K.

(Note: Google Benchmark's own `Complexity(oN)` coefficient is a least-squares fit
*through the origin* -- `time ≈ coef·N` with no intercept -- so it folds the fixed
cost into the coefficient and is NOT a clean per-call cost. The K=0-baseline
subtraction here is the correction.)

Benchmarks are discovered, not hardcoded: `cpp/<family>[/<K>]` pairs with
`rust/<family>[/<K>]`.
"""

import json
import sys

# Prefix in the JSON run_name -> column header. Order defines column order.
IMPLS = [("cpp", "C++"), ("rust", "Rust")]

# Which timing to report: "real_time" or "cpu_time".
TIME_KEY = "real_time"

# K at/above which per-call cost is treated as steady state for the aggregate
# table (below this the guest loop hasn't reached peak IPC; see BENCHMARK.md §1).
STEADY_MIN_K = 512

# Google Benchmark reports each row in its own `time_unit`; normalize to ns.
UNIT_NS = {"ns": 1.0, "us": 1e3, "ms": 1e6, "s": 1e9}

# Families that fell back to the `launch` baseline (no K=0 point present).
_FALLBACK = set()


def load(path):
    with open(path) as f:
        return json.load(f)


def parse_name(run_name):
    """'cpp/ldgr_index/512' -> ('cpp','ldgr_index',512); 'cpp/launch' -> ('cpp','launch',None)."""
    parts = run_name.split("/")
    if len(parts) == 2:
        return parts[0], parts[1], None
    if len(parts) == 3:
        try:
            return parts[0], parts[1], int(parts[2])
        except ValueError:
            return None
    return None


def collect(benchmarks):
    """Return (data, families): data[family][K][impl] = {"median": ns, "stddev": ns}."""
    impls = dict(IMPLS)
    data = {}
    families = []
    for b in benchmarks:
        if b.get("run_type") != "aggregate":
            continue
        agg = b.get("aggregate_name")
        if agg not in ("median", "stddev"):
            continue
        parsed = parse_name(b.get("run_name", ""))
        if not parsed:
            continue
        impl, family, k = parsed
        if impl not in impls:
            continue
        scale = UNIT_NS.get(b.get("time_unit", "ns"), 1.0)
        if family not in data:
            data[family] = {}
            families.append(family)
        data[family].setdefault(k, {}).setdefault(impl, {})[agg] = b[TIME_KEY] * scale
    return data, families


def median(data, family, k, impl):
    e = data.get(family, {}).get(k, {}).get(impl)
    return e.get("median") if e and "median" in e else None


def baseline(data, family, impl):
    """Fixed cost to subtract: the family's K=0 run, else the no-import `launch`."""
    b0 = median(data, family, 0, impl)
    if b0 is not None:
        return b0
    fb = median(data, "launch", None, impl)
    if fb is not None:
        _FALLBACK.add(family)
    return fb


def per_call_ns(data, family, k, impl):
    """(run(K) - baseline) / K in ns; None when K is 0/None or data is missing."""
    if not k:
        return None
    run = median(data, family, k, impl)
    base = baseline(data, family, impl)
    if run is None or base is None:
        return None
    return (run - base) / k


def fmt_us(entry):
    if not entry or "median" not in entry:
        return "—"
    m = entry["median"] / 1e3
    s = entry.get("stddev", 0.0) / 1e3
    return f"{m:.3g} ± {s:.2g}"


def fmt_ns(v):
    if v is None:
        return "—"
    return f"{v:.0f}" if abs(v) >= 100 else f"{v:.1f}"


def fmt_speed(cpp, rust):
    return f"{cpp / rust:.2f}x" if cpp and rust and rust > 0 else "—"


def emit(headers, rows):
    if not rows:
        return
    widths = [
        max(len(headers[i]), *(len(r[i]) for r in rows)) for i in range(len(headers))
    ]

    def line(cells):
        return "| " + " | ".join(c.ljust(widths[i]) for i, c in enumerate(cells)) + " |"

    print(line(headers))
    print("|" + "|".join("-" * (w + 2) for w in widths) + "|")
    for r in rows:
        print(line(r))


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "build/bench_result.json"
    data, families = collect(load(path).get("benchmarks", []))
    if not families:
        print(
            "No paired benchmarks found (run with --benchmark_repetitions>1).",
            file=sys.stderr,
        )
        sys.exit(1)

    def ks(family):
        return sorted(k for k in data[family] if k is not None) or [None]

    # --- Table 1: per-test (raw run time + per-call cost + per-call speedup) ---
    headers = [
        "test name",
        "C++, µs",
        "Rust, µs",
        "C++ ns/call",
        "Rust ns/call",
        "per-call ↑",
    ]
    rows = []
    for family in families:
        for k in ks(family):
            label = family if k is None else f"{family}/{k}"
            cpp_pc = per_call_ns(data, family, k, "cpp")
            rust_pc = per_call_ns(data, family, k, "rust")
            rows.append(
                [
                    label,
                    fmt_us(data[family].get(k, {}).get("cpp")),
                    fmt_us(data[family].get(k, {}).get("rust")),
                    fmt_ns(cpp_pc),
                    fmt_ns(rust_pc),
                    fmt_speed(cpp_pc, rust_pc),
                ]
            )
    emit(headers, rows)

    # --- Table 2: per-host-function summary (mean per-call cost, K >= STEADY_MIN_K) ---
    print()
    print(f"Per-host-function summary — mean per-call cost over K ≥ {STEADY_MIN_K}:")
    print()

    def mean_pc(family, impl, steady):
        vals = [per_call_ns(data, family, k, impl) for k in steady]
        vals = [v for v in vals if v is not None]
        return sum(vals) / len(vals) if vals else None

    rows2 = []
    for family in families:
        steady = [k for k in data[family] if k and k >= STEADY_MIN_K]
        if not steady:
            continue
        cpp_m = mean_pc(family, "cpp", steady)
        rust_m = mean_pc(family, "rust", steady)
        rows2.append([family, fmt_ns(cpp_m), fmt_ns(rust_m), fmt_speed(cpp_m, rust_m)])
    emit(["host-function contract", "C++ ns/call", "Rust ns/call", "per-call ↑"], rows2)

    if _FALLBACK:
        print(
            f"\nwarning: no K=0 baseline for {sorted(_FALLBACK)}; fell back to the "
            "no-import `launch` floor (rebuild the harness with `->Arg(0)` for an "
            "exact per-call cost).",
            file=sys.stderr,
        )


if __name__ == "__main__":
    main()
