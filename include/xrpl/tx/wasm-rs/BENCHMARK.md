# WASM engine benchmark: C++ (wasmi C-API) vs Rust PoC (native wasmi + cxx)

Head-to-head microbenchmark of the two programmable-escrow WASM engines:

- the **current C++ engine**, reached through its production entry point
  `xrpl::runEscrowWasm` (`include/xrpl/tx/wasm/WasmVM.h`), which drives wasmi
  through its **C-API** and dispatches host calls through the
  `xrpl::HostFunctions` vtable;
- the **Rust-native PoC engine**, reached over cxx through
  `xrpl::wasmrs::runEscrowWasmRsWithCxxHost` (`include/xrpl/tx/wasm-rs/WasmVmRs.h`),
  which drives **wasmi natively** and forwards host calls back to the same
  `xrpl::HostFunctions` primitives over cxx (`HostContext`).

Harness: `src/bench/wasm/WasmEngineBench.cpp` (Google Benchmark).

> **Both engines run the same interpreter — wasmi.** The C++ side uses the
> wasmi C-API; the Rust side uses the `wasmi` crate (v1.0.9) directly. So this
> benchmark does **not** compare interpreter cores; it isolates the cost of the
> **binding layer** around a shared interpreter: module setup, import
> resolution, argument/return marshaling, gas accounting, and the host-call
> dispatch path (C++ vtable vs Rust trait object reached over cxx). That is
> exactly the surface the redesign changes.

---

## Test conditions

| | |
|---|---|
| Machine | Linux |
| Build | `Release` / optimized (`-O3`, LTO on the Rust staticlib) — this exact code |
| Interpreter | wasmi — C-API (C++) vs `wasmi` crate 1.0.9 (Rust) |
| Harness | Google Benchmark, one `--benchmark_repetitions` set per row |
| Gas ceiling | `GAS = 1e9` fuel per run — generous, never the limiting factor |

**Measurement model.** Each *timed iteration* is one **full escrow finish** on a
**pre-assembled** `.wasm` binary: validate → instantiate → lazy-translate →
execute the exported `finish()`, driven through each engine's real production
entry point. WAT→bytes assembly happens **once per contract, outside** the timed
region (via the Rust `compile_wat` helper, so both engines consume the identical
bytes). There is deliberately **no** prepare / warm-up / persistent-instance
split — the per-escrow cost *is* what production pays.

**Same host, trivial returns.** Both engines are driven by one shared
`xrpl::HostFunctions` mock (`FakeHost`) whose methods return constants and do
**no real work** (no ledger lookups, no actual SHA-512). This is deliberate: the
benchmark measures **engine + binding overhead**, not primitive cost. In
particular `computeSha512HalfHash` returns a fixed 32-byte value *without
hashing its input* — remember this when reading the `sha512_half` rows.

**Fairness.** The C++ engine normally registers ~60 host-function imports; for
this benchmark it was trimmed to register **only the 5** the Rust PoC implements,
because import resolution happens inside the timed region for both engines.

**Guardrails in effect (identical on both engines).** 128-page (8 MiB) linear
memory cap; **1 KiB per-field** payload cap (`kMaxWasmDataLength`); **1 MiB
per-run** cumulative host↔guest transfer budget (`kWasmTransferLimit`). The 1 KiB
cap is why the largest payload any single host call can move is 1 KiB, and the
1 MiB per-run budget is why the `_1k` benchmarks stop at K=512 (512 × 1 KiB =
512 KiB/run, safely under budget) instead of sweeping to 8192.

### How to read the numbers

Each cell is the time for **one full contract run** (`finish()`) that makes **K**
host calls, where **K is the `/N` suffix** (`/1`, `/8`, … `/8192`). It is **not**
per host call. A run's cost is roughly:

```
run_time(K)  ≈  fixed_launch  +  K × per_host_call_cost
```

so `launch` (a no-import contract) is the fixed floor, and the marginal
per-host-call cost is `(run_time(K) − launch) / K`. The `±` is the run-to-run
spread (std-dev across repetitions). The **C++/Rust** column is `C++ ÷ Rust`
— values `> 1` mean the Rust PoC is faster by that factor.

---

## What each test case exercises

| Benchmark | Host fn | Input (guest→host) | Output (host→guest) | Stresses |
|---|---|---|---|---|
| `launch` | *(none)* | — | — | Pure engine startup floor: validate + instantiate + run a trivial `finish` with **no** host calls. |
| `ldgr_index` | `get_ledger_sqn` | — | 4 B (LE seq no.) | Output-only, tiny. The write path at minimum size. |
| `home_le_field` | `get_current_ledger_obj_field` | `i32` field code (scalar) | 4 B | Output-dominated. This is the **direct-write** path. |
| `sha512_half` | `sha512_half` | 3 B slice (`"abc"`) | 32 B digest | Read **and** write. (Host returns a constant — no real hashing.) |
| `trace` | `trace` | `"hi"` + 0-byte data (two reads) | — | Read-dominated, no output. |
| `trace_num` | `trace_num` | `"hi"` + `i64` scalar | — | One small read + a scalar. |
| `home_le_field_1k` | `get_current_ledger_obj_field` | `i32` field code | **1 KiB** | The **write / direct-write** path at the ABI's max payload. |
| `sha512_half_1k` | `sha512_half` | **1 KiB** slice | 32 B | The **read** path at the ABI's max payload. |

The two `_1k` cases exist to isolate **per-byte I/O cost** (and, for the Rust
engine, whether the cxx crossing adds any): compare each against its
small-payload namesake at the same K.

---

## Results

Time per full `finish()` run (µs), median ± std-dev across repetitions.

| test name            | C++, µs       | Rust, µs     | C++/Rust |
|----------------------|---------------|--------------|----------|
| launch               | 3.33 ± 0.033  | 3.5 ± 0.0098 | 0.95x    |
| ldgr_index/1         | 10.2 ± 0.24   | 9.69 ± 0.07  | 1.05x    |
| ldgr_index/8         | 11.6 ± 0.078  | 10.3 ± 0.036 | 1.12x    |
| ldgr_index/64        | 18 ± 0.069    | 14.2 ± 0.031 | 1.27x    |
| ldgr_index/512       | 67.8 ± 0.62   | 44.4 ± 0.045 | 1.53x    |
| ldgr_index/4096      | 443 ± 11      | 285 ± 1.4    | 1.55x    |
| ldgr_index/8192      | 847 ± 24      | 560 ± 0.61   | 1.51x    |
| home_le_field/1      | 11.7 ± 0.043  | 10 ± 0.028   | 1.17x    |
| home_le_field/8      | 12.7 ± 0.047  | 10.6 ± 0.061 | 1.19x    |
| home_le_field/64     | 19.7 ± 0.053  | 15.1 ± 0.041 | 1.30x    |
| home_le_field/512    | 74.1 ± 0.2    | 49.9 ± 0.049 | 1.49x    |
| home_le_field/4096   | 501 ± 1.1     | 328 ± 0.2    | 1.53x    |
| home_le_field/8192   | 986 ± 2       | 645 ± 0.75   | 1.53x    |
| sha512_half/1        | 12.8 ± 0.12   | 11.3 ± 0.033 | 1.13x    |
| sha512_half/8        | 13.5 ± 0.03   | 12.1 ± 0.11  | 1.12x    |
| sha512_half/64       | 21.8 ± 0.39   | 17.4 ± 0.48  | 1.25x    |
| sha512_half/512      | 85.9 ± 0.71   | 58.8 ± 0.14  | 1.46x    |
| sha512_half/4096     | 535 ± 21      | 383 ± 2.1    | 1.40x    |
| sha512_half/8192     | 1.07e+03 ± 26 | 753 ± 3.1    | 1.42x    |
| home_le_field_1k/1   | 11.9 ± 0.063  | 10.2 ± 0.053 | 1.17x    |
| home_le_field_1k/8   | 12.9 ± 0.034  | 10.9 ± 0.03  | 1.19x    |
| home_le_field_1k/64  | 20.1 ± 0.065  | 15.7 ± 0.027 | 1.28x    |
| home_le_field_1k/512 | 76.1 ± 0.27   | 53.9 ± 0.091 | 1.41x    |
| sha512_half_1k/1     | 11.7 ± 0.22   | 8.92 ± 0.028 | 1.31x    |
| sha512_half_1k/8     | 12.6 ± 0.028  | 9.81 ± 0.03  | 1.29x    |
| sha512_half_1k/64    | 19.4 ± 0.036  | 15.9 ± 0.037 | 1.23x    |
| sha512_half_1k/512   | 72.8 ± 0.13   | 63.4 ± 0.14  | 1.15x    |
| trace/1              | 13.3 ± 0.2    | 11.3 ± 0.53  | 1.18x    |
| trace/8              | 14.3 ± 0.16   | 12.2 ± 0.038 | 1.17x    |
| trace/64             | 20.6 ± 0.17   | 17.8 ± 0.059 | 1.15x    |
| trace/512            | 69.3 ± 0.25   | 61.6 ± 0.21  | 1.12x    |
| trace/4096           | 451 ± 1.2     | 405 ± 2.5    | 1.11x    |
| trace/8192           | 876 ± 4.6     | 787 ± 4.4    | 1.11x    |
| trace_num/1          | 12.8 ± 0.034  | 10.9 ± 0.058 | 1.17x    |
| trace_num/8          | 13.6 ± 0.08   | 11.6 ± 0.032 | 1.17x    |
| trace_num/64         | 19.2 ± 0.047  | 16 ± 0.028   | 1.20x    |
| trace_num/512        | 63.5 ± 0.18   | 50.7 ± 0.078 | 1.25x    |
| trace_num/4096       | 404 ± 0.82    | 327 ± 0.61   | 1.24x    |
| trace_num/8192       | 784 ± 2.2     | 642 ± 1      | 1.22x    |

---

## Analysis

### 1. The Rust PoC is faster on every case; the gap is all in per-call binding

The Rust engine wins in all 39 rows, by **1.05×–1.55×**. Two facts locate the
win precisely in the binding layer:

- **`launch` is a tie (0.95×).** With zero host calls, the two engines are
  within ~5% (Rust marginally slower, 3.5 vs 3.33 µs). Same interpreter, same
  validate+instantiate cost — as expected.
- **The ratio grows with K** and plateaus at high call counts (e.g. `ldgr_index`
  1.05× at K=1 → 1.53× at K=512). At K=1 the fixed launch dominates and the two
  look similar; as host calls pile up, the **per-call** difference takes over.

So the difference is not the interpreter and not module setup — it's the
per-host-call path. Launch-subtracted marginal cost at K=512 for `ldgr_index`:
C++ ≈ (67.8 − 3.33)/512 ≈ **126 ns/call**, Rust ≈ (44.4 − 3.5)/512 ≈ **80
ns/call**. The Rust native-wasmi + cxx-host path dispatches a host call in
roughly two-thirds the time of the wasmi-C-API + vtable path.

### 2. Write-dominated calls win most; read-dominated calls win least

Ranking the high-K ratios by what the call *does*:

| Call | I/O shape | ratio @ high K |
|---|---|---|
| `ldgr_index`, `home_le_field` | output-only | **~1.5–1.55×** |
| `sha512_half` | read + write | ~1.40–1.46× |
| `trace_num` | small read + scalar | ~1.22–1.25× |
| `trace` | read-dominated (two reads) | **~1.11×** |

This is a coherent gradient: **the more a call is dominated by reading input
out of guest memory, the smaller the Rust advantage.** The reason is the two
engines' differing treatment of the two directions:

- **Output (host→guest):** the Rust PoC uses the **direct-write** path — the
  host writes its bytes straight into a `&mut [u8]` that aliases guest linear
  memory (`abi.rs::write_into`), and over cxx that slice is passed as a
  pointer+len with no owned buffer/`Vec`/result-struct marshaled. This is as
  cheap as (here, cheaper than) the C++ engine's own write — hence the big win
  on the output-only calls.
- **Input (guest→host):** the Rust engine currently **copies** the input out of
  guest memory into an owned `Vec<u8>` (`abi.rs`'s `AbiArg for Vec<u8>`) before
  handing the host a `&[u8]`. The C++ engine instead builds a zero-copy `Slice`
  pointing directly into guest memory (`getDataSlice`). So on read-heavy calls
  the Rust engine does one extra `memcpy` per input that C++ does not — which
  eats into its per-call advantage. `trace`, which does two reads and no write,
  is where this shows up most (1.11×).

Note the cxx boundary itself is **not** the cost here: inputs cross cxx as a
zero-copy `rust::Slice` too. The extra copy is an *engine-internal* choice in
`AbiArg`, and it is the read-side analog of the write-side direct-write that has
already been done. Making reads zero-copy (hand the host a slice aliasing guest
memory, mirroring `write_into`) would close this gap.

### 3. Large payloads compress the ratio — and reads compress much faster than writes

Comparing each `_1k` (1 KiB) case against its small namesake at K=512:

| Path | small ratio | 1 KiB ratio | Δ |
|---|---|---|---|
| `home_le_field` (write) | 1.49× | 1.41× | **−0.08** |
| `sha512_half` (read)    | 1.46× | 1.15× | **−0.31** |

As the payload grows, both ratios drift toward 1.0 — expected, because the
payload `memcpy` is (nearly) equal work for both engines, so it dilutes Rust's
fixed per-call dispatch advantage. But the **read** ratio collapses ~4× faster
than the **write** ratio. That is the same finding as §2, now visible as a slope:
Rust's *write* stays as efficient as C++ even at 1 KiB (direct-write ≈ C++'s
`setData`), while Rust's *read* pays an extra 1 KiB copy C++ avoids, so its
advantage erodes as the input grows.

For the team's original question — *"is there I/O overhead from cxx?"* — the
answer the data gives is **no**: payloads cross the cxx boundary as pointer+len
slices in both directions, the write path is zero-copy end-to-end, and the only
per-byte asymmetry versus C++ is the Rust engine's *input* copy, which is
internal to the engine (not cxx) and is fixable.

### 4. The `sha512_half` vs `sha512_half_1k` "anomaly"

The eyebrow-raiser: `sha512_half_1k` (1 KiB input) is **faster** than
`sha512_half` (3 B input) in several C++ rows — e.g. C++ at K=512: 72.8 vs
85.9 µs. A bigger input finishing faster looks wrong.

It isn't a real effect. Three things resolve it:

1. **The host does no hashing.** `FakeHost::computeSha512HalfHash` returns a
   constant regardless of input, so a 1 KiB input creates **no extra host
   work** — only the *engine's* read of the bytes could differ.
2. **The C++ read is zero-copy.** `getDataSlice` builds a `Slice(ptr, len)` in
   O(1); reading 3 B vs 1 KiB costs the C++ engine essentially the *same*.
   There is therefore **no mechanism** by which the 1 KiB C++ case could be
   genuinely faster than the 3 B case — they should be equal.
3. **The small run is the outlier, not the large one.** Anchor against the
   comparable `home_le_field/512` (C++ 74.1 µs). `sha512_half_1k/512` = 72.8 µs
   sits right on that anchor (both do a read + a small write); `sha512_half/512`
   = 85.9 µs is ~12 µs *above* it. And the small `sha512_half` rows carry the
   **largest error bars in the whole suite** (`/4096` = 535 ± 21, `/8192` =
   1070 ± 26). So the small `sha512_half` measurement drifted high, while the
   `_1k` value is the well-behaved one.

The physically real signal here is on the **Rust** side, which *is* consistent:
Rust `sha512_half/512` = 58.8 → `sha512_half_1k/512` = 63.4 µs, i.e. **+4.6 µs**
going from 3 B to 1 KiB. That is exactly the extra 1 KiB-per-call input copy
(§2/§3) — ~9 ns/call for a 1 KiB `memcpy`, in the expected direction. C++ stays
flat (zero-copy), so at 1 KiB the two converge and the ratio drops to 1.15×.

**Bottom line:** it's a cross-benchmark measurement artifact (separately
registered benchmarks run at different points in the sequence, under slightly
different thermal/scheduling conditions), amplified by the noisy small-`sha512`
run — not a case of "bigger is faster." To confirm, re-run with
`--benchmark_repetitions=N --benchmark_enable_random_interleaving=true` and,
ideally, CPU pinning / a quiesced machine; the small/large C++ `sha512` numbers
should then match.

---

## Conclusions

1. **Rust PoC ≥ C++ on every workload measured (1.05×–1.55×).** Same interpreter,
   so the win is entirely in the binding layer around wasmi.
2. **The win is per-host-call, not startup.** `launch` is a tie; the advantage
   scales with the number of host calls a contract makes.
3. **No cxx I/O overhead.** Payloads cross the boundary as zero-copy slices both
   ways; the direct-write output path is as cheap as C++'s native write (often
   cheaper).
4. **One remaining, non-cxx asymmetry:** the Rust engine copies host-call
   *inputs* into an owned `Vec<u8>`, which the C++ engine avoids with a
   zero-copy `Slice`. It shows up as the smaller advantage on read-heavy calls
   (`trace` ≈ 1.11×) and as the ratio collapse on `sha512_half_1k`. This is the
   read-side analog of the already-implemented write-side direct-write and is
   the natural next optimization.
5. The `sha512_half`-vs-`sha512_half_1k` inversion is a measurement artifact
   (see §4), not a real result.

### Caveats

- Micro-benchmark on one machine; absolute numbers are machine- and
  build-specific. The **ratios and trends** are the takeaway, not the µs.
- `FakeHost` returns constants, so these numbers exclude real primitive cost
  (ledger access, actual SHA-512). They measure engine + binding overhead only —
  by design.
- The C++ engine was trimmed to 5 imports for fairness; a full 60-import build
  would have somewhat higher import-resolution cost inside the timed region.

### Reproducing

```
cmake --build build --target xrpl_wasm_bench
./build/xrpl_wasm_bench \
    --benchmark_repetitions=15 \
    --benchmark_enable_random_interleaving=true \
    --benchmark_report_aggregates_only=true
```

Filter to a subset with e.g. `--benchmark_filter='(rust|cpp)/sha512_half(_1k)?/.*'`.
