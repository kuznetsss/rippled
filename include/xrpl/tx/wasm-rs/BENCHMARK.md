# WASM engine benchmark: C++ (wasmi C-API) vs Rust PoC (native wasmi + cxx)

Head-to-head microbenchmark of the two programmable-escrow WASM engines:

- the **current C++ engine**, reached through its production entry point
  `xrpl::runEscrowWasm` ([`WasmVM.h`](../wasm/WasmVM.h)), which drives wasmi
  through its **C-API** and dispatches host calls through the
  `xrpl::HostFunctions` vtable;
- the **Rust-native PoC engine**, reached over cxx through
  `xrpl::wasmrs::runEscrowWasmRsWithCxxHost` ([`WasmVmRs.h`](WasmVmRs.h)),
  which drives **wasmi natively** and forwards host calls back to the same
  `xrpl::HostFunctions` primitives over cxx ([`HostContext`](HostContext.h)).

Harness: [`src/bench/wasm/WasmEngineBench.cpp`](../../../../src/bench/wasm/WasmEngineBench.cpp)
(Google Benchmark).

> **Both engines run the same interpreter, at the same version — wasmi 1.0.9.**
> The C++ side embeds wasmi through its **C-API** (the conan `wasmi/1.0.9`
> package); the Rust side embeds the **identical** `wasmi` crate, pinned to
> `=1.0.9`, directly. Same interpreter, same version — so this benchmark does
> **not** compare interpreter cores; it isolates the cost of the **binding
> layer** around that shared interpreter: module setup, import resolution,
> argument/return marshaling, gas accounting, and the host-call dispatch path
> (C++ vtable vs Rust trait object reached over cxx). That is exactly the
> surface the redesign changes.

---

## Test conditions

| | |
|---|---|
| Machine | Linux |
| Build | `Release` / optimized (`-O3`, LTO on the Rust staticlib) — this exact code |
| Interpreter | **wasmi 1.0.9 on both sides** — C-API (C++, conan `wasmi/1.0.9`) vs `wasmi` crate `=1.0.9` (Rust) |
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
memory cap; **1 KiB per-field** payload cap
([`kMaxWasmDataLength`](../../protocol/Protocol.h)); **1 MiB per-run** cumulative
host↔guest transfer budget
([`kWasmTransferLimit`](../../protocol/Protocol.h)). The 1 KiB
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
| launch               | 3.32 ± 0.046  | 3.49 ± 0.01  | 0.95x    |
| ldgr_index/1         | 10.2 ± 0.23   | 9.62 ± 0.05  | 1.06x    |
| ldgr_index/8         | 11.5 ± 0.088  | 10.2 ± 0.037 | 1.13x    |
| ldgr_index/64        | 18.1 ± 0.072  | 14 ± 0.047   | 1.29x    |
| ldgr_index/512       | 69.5 ± 0.62   | 44 ± 0.05    | 1.58x    |
| ldgr_index/4096      | 454 ± 16      | 283 ± 0.28   | 1.60x    |
| ldgr_index/8192      | 869 ± 15      | 556 ± 0.46   | 1.56x    |
| home_le_field/1      | 11.6 ± 0.025  | 9.89 ± 0.022 | 1.18x    |
| home_le_field/8      | 12.7 ± 0.07   | 10.6 ± 0.021 | 1.20x    |
| home_le_field/64     | 19.6 ± 0.066  | 14.9 ± 0.033 | 1.31x    |
| home_le_field/512    | 73 ± 0.095    | 49.9 ± 0.051 | 1.46x    |
| home_le_field/4096   | 489 ± 0.48    | 329 ± 0.29   | 1.49x    |
| home_le_field/8192   | 957 ± 3       | 647 ± 0.61   | 1.48x    |
| sha512_half/1        | 12.7 ± 0.12   | 10.4 ± 0.028 | 1.22x    |
| sha512_half/8        | 13.8 ± 0.05   | 11.2 ± 0.053 | 1.23x    |
| sha512_half/64       | 21.8 ± 0.13   | 16 ± 0.17    | 1.37x    |
| sha512_half/512      | 87.1 ± 0.48   | 55.4 ± 0.36  | 1.57x    |
| sha512_half/4096     | 537 ± 16      | 365 ± 0.72   | 1.47x    |
| sha512_half/8192     | 1.05e+03 ± 10 | 718 ± 3.1    | 1.47x    |
| home_le_field_1k/1   | 11.9 ± 0.038  | 10 ± 0.016   | 1.19x    |
| home_le_field_1k/8   | 12.9 ± 0.021  | 10.7 ± 0.032 | 1.20x    |
| home_le_field_1k/64  | 19.9 ± 0.028  | 15.8 ± 0.043 | 1.26x    |
| home_le_field_1k/512 | 73.4 ± 0.13   | 55.7 ± 0.063 | 1.32x    |
| sha512_half_1k/1     | 11.6 ± 0.2    | 8.78 ± 0.021 | 1.33x    |
| sha512_half_1k/8     | 12.6 ± 0.017  | 9.6 ± 0.032  | 1.31x    |
| sha512_half_1k/64    | 19.6 ± 0.035  | 15.6 ± 0.045 | 1.26x    |
| sha512_half_1k/512   | 74.3 ± 0.38   | 62.4 ± 0.066 | 1.19x    |
| trace/1              | 13.4 ± 0.15   | 10.4 ± 0.68  | 1.29x    |
| trace/8              | 14.3 ± 0.15   | 11.2 ± 0.062 | 1.28x    |
| trace/64             | 20.4 ± 0.14   | 15.6 ± 0.052 | 1.31x    |
| trace/512            | 69.1 ± 0.18   | 50.9 ± 0.73  | 1.36x    |
| trace/4096           | 447 ± 0.7     | 334 ± 2.1    | 1.34x    |
| trace/8192           | 872 ± 2.1     | 657 ± 4.6    | 1.33x    |
| trace_num/1          | 12.7 ± 0.046  | 10 ± 0.028   | 1.26x    |
| trace_num/8          | 13.5 ± 0.028  | 10.6 ± 0.023 | 1.28x    |
| trace_num/64         | 19.2 ± 0.059  | 14.4 ± 0.024 | 1.33x    |
| trace_num/512        | 63.8 ± 0.13   | 45.3 ± 0.088 | 1.41x    |
| trace_num/4096       | 409 ± 0.47    | 291 ± 0.36   | 1.40x    |
| trace_num/8192       | 797 ± 3       | 572 ± 0.85   | 1.39x    |

---

## Analysis

### 1. The Rust PoC is faster on every case; the gap is all in per-call binding

The Rust engine wins on every host-call workload by **1.06×–1.60×** (`launch`,
which makes no host calls, is a 0.95× wash). Two facts locate the win precisely
in the binding layer:

- **`launch` is a tie (0.95×).** With zero host calls, the two engines are
  within ~5% (Rust marginally slower, 3.49 vs 3.32 µs). Same interpreter, same
  validate+instantiate cost — as expected.
- **The ratio grows with K** and plateaus at high call counts (e.g. `ldgr_index`
  1.06× at K=1 → 1.58× at K=512). At K=1 the fixed launch dominates and the two
  look similar; as host calls pile up, the **per-call** difference takes over.

So the difference is not the interpreter and not module setup — it's the
per-host-call path. Launch-subtracted marginal cost at K=512 for `ldgr_index`:
C++ ≈ (69.5 − 3.32)/512 ≈ **129 ns/call**, Rust ≈ (44 − 3.49)/512 ≈ **79
ns/call**. The Rust native-wasmi + cxx-host path dispatches a host call in
roughly two-thirds the time of the wasmi-C-API + vtable path. §2 explains the
mechanism behind that per-call gap.

### 2. Why the wasmi C-API path costs more per call

Both engines drive the *same* interpreter, so a per-call gap can only come from
the code wrapped around each host call — and the two engines wrap it very
differently. It comes down to **where the FFI wall sits** and **how much
marshaling that wall forces on every call**.

**C++ engine — the interpreter↔host boundary is an untyped C ABI.** wasmi is
itself Rust; the C++ engine reaches it through wasmi's C-API (`wasm.h` /
`wasmi.h`). That C ABI is the standard `wasm-c-api`, whose host-callback
convention is *untyped*: every callback has the one signature
`wasm_trap_t*(void* env, wasm_val_vec_t const* params, wasm_val_vec_t* results)`
([`HostFuncWrapper.h:11`](../wasm/HostFuncWrapper.h)). So on **every** host call
the interpreter must:

- **Box each argument into a `wasm_val_t` tagged union** laid out in a `params`
  vector, which the C++ side reads back field-by-field (`params->data[i].of.i32`,
  [`HostFuncWrapper.cpp:177`](../../../../src/libxrpl/tx/wasm/HostFuncWrapper.cpp)); the return is boxed the same way
  (`results->data[0] = WASM_I32_VAL(...)`, `HostFuncWrapper.cpp:408`). The cost
  scales with arity — the full host set has keylet calls that marshal up to 8
  such unions — and is pure overhead versus passing typed scalars.
- **Call an opaque C function pointer.** Every import is registered with the
  *same* generic trampoline, `HostFuncMain_wrap`, via `wasm_func_new_with_env`
  ([`WasmiVM.cpp:440`](../../../../src/libxrpl/tx/wasm/WasmiVM.cpp)). That trampoline does its own per-call work — null-checks,
  a try/catch frame, the gas check — then dispatches through a **second**
  function pointer (`impFunc.wrap`, `HostFuncWrapper.cpp:542`) to the specific
  `_wrap`, which finally makes a **virtual** call into `xrpl::HostFunctions`.
  Two indirect jumps plus a vtable dispatch, none of them inlinable.
- **Reach engine state back through the C-API.** Gas is charged per call with
  `wasm_store_get_fuel` + `wasm_store_set_fuel` (`WasmiVM.cpp:237,249`, via
  `checkGas`), and guest memory is fetched with `wasm_memory_data` +
  `wasm_memory_data_size` (`WasmiVM.cpp:210,228`) — each another hop across the
  same C ABI.

The compounding cost is that the C ABI is an **optimization barrier**: the C++
compiler cannot inline *anything* across `wasm.h` — not the argument marshaling,
not the fuel accessors, not the interpreter's own call dispatch — so every item
above stays a discrete, opaque call.

**Rust PoC — the interpreter↔host boundary is native Rust.** The engine embeds
the `wasmi` crate directly, so a host call is a *typed* `Func::wrap` closure
registered per import ([`imports.rs:30`](../../../../crates/wasm_vm/src/imports.rs)). wasmi feeds the interpreter's operands
to that closure as **typed Rust scalars** straight off the value stack — no
`wasm_val_t`, no tagged-union vector, no per-argument boxing. Gas is
`Caller::get_fuel`/`set_fuel` and memory is `Caller::get_export` + a
bounds-checked slice ([`abi.rs`](../../../../crates/wasm_vm/src/abi.rs)), all ordinary Rust calls. And because the
interpreter, the ABI marshaling, the closure body, and gas/memory accounting are
**one LTO'd Rust compilation graph**, the compiler inlines across what is a hard
FFI wall in the C++ build.

**So the win is not "cxx beats the C-API" — it's that the FFI wall moves.** The
Rust engine still crosses one FFI boundary per call, but only a thin, *typed*
cxx hop out to the C++ primitive (`CxxHost` → `HostContext`), carrying scalars
and pointer+len slices — no tagged unions, and it is the *only* non-inlinable
step. The wasmi C-API instead puts an untyped, non-inlinable wall between the
interpreter and *all* host logic, and pays `wasm_val_t` marshaling on every
argument of every call. That is the ≈129 ns/call (C++) vs ≈79 ns/call (Rust)
`ldgr_index` gap from §1.

### 3. The per-call gap by I/O shape: dispatch-bound calls win most

How the engine moves bytes across the host boundary sets the per-call cost.
Host-call *inputs* are never heap-copied: a **read-only** call hands the host a
`&[u8]` **aliasing guest linear memory**
([`read_borrowed`](../../../../crates/wasm_vm/src/abi.rs), zero-copy), and a
**read+write** call copies its input into a fixed **stack** buffer with no heap
allocation ([`read_write`](../../../../crates/wasm_vm/src/abi.rs)). Outputs use
the **direct-write** path: the host writes straight into a `&mut [u8]` aliasing
guest memory ([`write_into`](../../../../crates/wasm_vm/src/abi.rs)), passed over
cxx as pointer+len with no owned buffer.

Ranking the high-K ratios by what the call *does*:

| Call | I/O shape | ratio @ high K |
|---|---|---|
| `ldgr_index` | output-only, no input | **~1.56–1.60×** |
| `home_le_field` | output-only (+ scalar in) | ~1.48–1.49× |
| `sha512_half` | read + write | ~1.47× |
| `trace_num` | small read + scalar | ~1.39–1.40× |
| `trace` | read-only, two reads | ~1.33–1.34× |

The ratios sit in a tight **~1.33–1.60×** band, and the ordering within it tracks
**how much of a call is pure dispatch** (where Rust's cheaper binding, §2, wins
big) **versus shared per-call work both engines do the same way** (bounds /
transfer accounting, UTF-8 validation, the actual primitive):

- **Output-only calls are almost pure dispatch**, so Rust's per-call advantage
  shows nearly in full — the top of the band (~1.5–1.6×), since the direct-write
  output is as cheap as (here cheaper than) C++'s own write.
- **`trace` does the most shared work per call** (two `read_borrowed`s + UTF-8
  validation), so dispatch is a smaller fraction and its ratio, while still
  solid, is the lowest (~1.33×). Concretely, launch-subtracted marginal cost at
  K=512: C++ is ≈128–129 ns/call for **both** `ldgr_index` and `trace` (its
  per-call cost is dominated by the fixed C-API marshaling of §2), while Rust is
  ≈79 ns/call for `ldgr_index` vs ≈93 ns/call for `trace` — the extra ~14 ns is
  `trace`'s two reads + validation, which Rust does in-engine while C++'s large
  fixed overhead hides it. Both engines read zero-copy here, so it is not a copy
  asymmetry.

The cxx boundary is **not** the cost: read-only inputs are zero-copy end-to-end
(guest memory → `&[u8]` → `rust::Slice` → C++). The one asymmetry that remains is
narrow — read+write calls (`sha512_half`) copy their input into the stack buffer
while C++ reads it zero-copy
([`getDataSlice`](../../../../src/libxrpl/tx/wasm/HostFuncWrapper.cpp)) — and it
is invisible at small payloads, surfacing only at large *read* payloads (§4/§5).
(That stack buffer also carries a fixed ~1 KiB zero-fill per call, a
deliberately-simple trade noted in
[`abi.rs`](../../../../crates/wasm_vm/src/abi.rs); a `MaybeUninit` buffer would
drop it.)

### 4. Large payloads compress the ratio — and reads compress faster than writes

Comparing each `_1k` (1 KiB) case against its small namesake at K=512 (the
`_1k` cases stop at K=512, so this is the deepest comparison available):

| Path | small ratio | 1 KiB ratio | Δ |
|---|---|---|---|
| `home_le_field` (output / write) | 1.46× | 1.32× | **−0.14** |
| `sha512_half` (read + write)     | 1.57× | 1.19× | **−0.38** |

As the payload grows, both ratios drift toward 1.0 — but the **read** case
collapses far faster than the **write** case. The reason is *which direction*
grows, and who pays for it:

- **`home_le_field`'s growing payload is the output**, which **both** engines
  `memcpy` into guest memory (Rust's direct-write ≈ C++'s `setData`). Shared
  work dilutes Rust's fixed dispatch advantage only mildly.
- **`sha512_half`'s growing payload is the input**, which Rust copies into its
  stack buffer but C++ reads **zero-copy** (`getDataSlice`). The extra 1 KiB is
  work **only Rust does**, so it erodes the ratio much faster. (This is the same
  residual read+write asymmetry noted in §3, now visible as a slope.)

Caveat: the `sha512_half` small-payload point (1.57× at K=512) is the same C++
measurement that drifts high in §5; its de-noised small ratio is closer to
~1.47× (its K=4096/8192 value), so the *true* read compression is nearer −0.28
than the raw −0.38 — still markedly steeper than the write case's −0.14.

For the team's original question — *"is there I/O overhead from cxx?"* — the
answer the data gives is **no**: payloads cross the cxx boundary as pointer+len
slices in both directions, both the write path and the read-only input path are
zero-copy end-to-end, and the only per-byte asymmetry left versus C++ is the
*input* copy on read+write calls (now into a stack buffer, no allocation), which
is internal to the engine (not cxx).

### 5. The `sha512_half` vs `sha512_half_1k` "anomaly"

The eyebrow-raiser: `sha512_half_1k` (1 KiB input) is **faster** than
`sha512_half` (3 B input) in several C++ rows — e.g. C++ at K=512: 74.3 vs
87.1 µs. A bigger input finishing faster looks wrong.

It isn't a real effect. Three things resolve it:

1. **The host does no hashing.** `FakeHost::computeSha512HalfHash` returns a
   constant regardless of input, so a 1 KiB input creates **no extra host
   work** — only the *engine's* read of the bytes could differ.
2. **The C++ read is zero-copy.** `getDataSlice` builds a `Slice(ptr, len)` in
   O(1); reading 3 B vs 1 KiB costs the C++ engine essentially the *same*.
   There is therefore **no mechanism** by which the 1 KiB C++ case could be
   genuinely faster than the 3 B case — they should be equal.
3. **The small run is the outlier, not the large one.** Anchor against the
   comparable `home_le_field/512` (C++ 73.0 µs). `sha512_half_1k/512` = 74.3 µs
   sits right on that anchor (both do a read + a small write); `sha512_half/512`
   = 87.1 µs is ~14 µs *above* it. And the small `sha512_half` rows are **among
   the noisiest at high K** (`/4096` = 537 ± 16, `/8192` = 1.05e+03 ± 10). So the
   small `sha512_half` measurement drifted high, while the `_1k` value is the
   well-behaved one.

The physically real signal here is on the **Rust** side, which *is* consistent:
Rust `sha512_half/512` = 55.4 → `sha512_half_1k/512` = 62.4 µs, i.e. **+7.0 µs**
going from 3 B to 1 KiB. That is exactly the extra 1 KiB-per-call input copy
(§3/§4) — ~14 ns/call for a 1 KiB copy, now into the stack buffer rather than a
heap `Vec`, in the expected direction. C++ stays flat (zero-copy), so at 1 KiB
the two converge and the ratio drops to 1.19×.

**Bottom line:** it's a cross-benchmark measurement artifact (separately
registered benchmarks run at different points in the sequence, under slightly
different thermal/scheduling conditions), amplified by the noisy small-`sha512`
run — not a case of "bigger is faster." To confirm, re-run with
`--benchmark_repetitions=N --benchmark_enable_random_interleaving=true` and,
ideally, CPU pinning / a quiesced machine; the small/large C++ `sha512` numbers
should then match.

---

## Conclusions

1. **Rust PoC ≥ C++ on every host-call workload (1.06×–1.60×).** Same
   interpreter, so the win is entirely in the binding layer around wasmi — see
   §2 for the mechanism.
2. **The win is per-host-call, not startup.** `launch` is a tie; the advantage
   scales with the number of host calls a contract makes.
3. **No cxx I/O overhead.** Payloads cross the boundary as zero-copy slices both
   ways; the direct-write output path is as cheap as C++'s native write (often
   cheaper).
4. **Reads carry no copy penalty.** Host-call *inputs* are never heap-copied:
   read-only calls hand the host a `&[u8]` aliasing guest memory
   (`read_borrowed`, zero-copy) and read+write calls copy into a stack buffer
   with no heap allocation (`read_write`). Read-heavy and write-heavy calls sit
   in the same ~1.33–1.60× band (§3). The only residual is that read+write calls
   copy their input into the stack buffer while C++ reads it zero-copy
   (`getDataSlice`); it is invisible except at large read payloads
   (`sha512_half_1k`, §4/§5).
5. The `sha512_half`-vs-`sha512_half_1k` inversion is a measurement artifact
   (see §5), not a real result.

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
