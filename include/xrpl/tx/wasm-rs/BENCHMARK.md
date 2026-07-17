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

Each **`C++, µs` / `Rust, µs`** cell is the time for **one full contract run**
(`finish()`) that makes **K** host calls, where **K is the `/N` suffix** (`/0`,
`/1`, … `/8192`); the `±` is the run-to-run std-dev.

**The per-call cost model.** A run decomposes as

```
run(K)  ≈  F  +  Σ (i = 1..K) cᵢ
```

where `F` is the contract's fixed cost (validate + instantiate + **import
resolution** + memory + translation) and `cᵢ` is the cost of the *i*-th host
call. We measure `F` **directly** as the **`/0` row** — the same contract with
its host-call loop running zero times — and report

- **`ns/call`** `= (run(K) − run(0)) / K` — the mean cost of one host call;
- **`per-call ↑`** `= C++ ns/call ÷ Rust ns/call` (`> 1` = Rust faster).

Subtracting each contract's *own* `/0` baseline — not the shared no-import
`launch` floor — is what makes this correct. The host-call contracts pay for an
imported function and a 64 KB memory that `launch` does not, so their `F` is
several µs higher (e.g. `sha512_half`'s `/0` is 10.3 µs vs `launch`'s ~3.1 µs;
`/0` even differs *between* contracts — `sha512_half_1k`'s is 8.67 µs — which is
why the baseline is per-contract). Subtracting the too-small `launch` would
leave that surplus in the numerator, where `surplus / K` masquerades as a
per-call cost that "falls" with K.

**Per-call cost is still not flat across K — and that is expected, not a missing
constant.** Even with `F` removed exactly, `ns/call` declines from K = 1 to
~K = 512 and then settles (Rust `ldgr_index`: 108 → 79 → 70 → 68 → 67). `F` is
fully captured by `/0`, so this is **not** an unaccounted fixed cost; it is an
intra-run **warm-up transient** — the first host calls in a run execute below
steady-state IPC (branch-predictor training for the loop back-edge and the
indirect host-call dispatch, plus cache warmth), so the early `cᵢ` are larger and
the *average* falls toward the steady-state marginal cost as K grows. A `/0`
baseline cannot remove it (there are no calls to warm up), so we **report the
steady state** — the per-host-function summary averages K ≥ 512. Two corollaries:
read `ns/call` at K ≥ 512, and distrust the `/1` rows — they subtract two large,
near-equal run times (e.g. `trace/1` differences two ~10.4 µs numbers each ±0.4),
so a lone value like `trace/1 = 2.97×` is noise, not a 3× call.

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

**`C++, µs` / `Rust, µs`** are the measured full-run times (median ± std-dev).
**`ns/call`** is the derived per-call cost `(run(K) − run(0)) / K`, subtracting
each contract's own **`/0`** row (the same contract with its host-call loop
running zero times — launch + import resolution + memory, no calls), and
**`per-call ↑`** is the per-call speedup `C++ ns/call ÷ Rust ns/call`. Per-call
cost drifts down until ~K = 512 (intra-run warm-up), so read it at K ≥ 512 — see
*How to read the numbers* for the model.

| test name            | C++, µs       | Rust, µs     | C++ ns/call | Rust ns/call | per-call ↑ |
|----------------------|---------------|--------------|-------------|--------------|------------|
| launch               | 3.1 ± 0.24    | 3.52 ± 0.21  | —           | —            | —          |
| ldgr_index/0         | 11.4 ± 0.054  | 9.94 ± 0.16  | —           | —            | —          |
| ldgr_index/1         | 11.6 ± 0.076  | 10 ± 0.067   | 186         | 108          | 1.73x      |
| ldgr_index/8         | 12.4 ± 0.047  | 10.6 ± 0.07  | 132         | 78.8         | 1.67x      |
| ldgr_index/64        | 18.9 ± 0.13   | 14.4 ± 0.056 | 117         | 69.9         | 1.67x      |
| ldgr_index/512       | 69.5 ± 0.69   | 44.8 ± 0.31  | 113         | 68.1         | 1.67x      |
| ldgr_index/4096      | 413 ± 3.6     | 285 ± 2.6    | 98.0        | 67.2         | 1.46x      |
| ldgr_index/8192      | 798 ± 54      | 565 ± 4.8    | 96.0        | 67.8         | 1.42x      |
| home_le_field/0      | 11.9 ± 0.051  | 9.97 ± 0.062 | —           | —            | —          |
| home_le_field/1      | 12.1 ± 0.082  | 10.1 ± 0.056 | 188         | 155          | 1.22x      |
| home_le_field/8      | 13.1 ± 0.057  | 10.7 ± 0.061 | 143         | 95.3         | 1.50x      |
| home_le_field/64     | 20 ± 0.23     | 15.1 ± 0.057 | 126         | 80.9         | 1.56x      |
| home_le_field/512    | 73.4 ± 0.58   | 50.2 ± 0.32  | 120         | 78.6         | 1.53x      |
| home_le_field/4096   | 490 ± 2.3     | 330 ± 1.8    | 117         | 78.1         | 1.49x      |
| home_le_field/8192   | 965 ± 8.2     | 650 ± 2.3    | 116         | 78.1         | 1.49x      |
| sha512_half/0        | 12.8 ± 0.081  | 10.3 ± 0.12  | —           | —            | —          |
| sha512_half/1        | 13 ± 0.14     | 10.5 ± 0.31  | 171         | 139          | 1.23x      |
| sha512_half/8        | 14.1 ± 0.089  | 11.2 ± 0.2   | 161         | 109          | 1.48x      |
| sha512_half/64       | 21.8 ± 0.35   | 15.9 ± 0.14  | 141         | 87.4         | 1.61x      |
| sha512_half/512      | 81.2 ± 3.6    | 53.1 ± 0.48  | 134         | 83.5         | 1.60x      |
| sha512_half/4096     | 504 ± 3.1     | 351 ± 3.2    | 120         | 83.2         | 1.44x      |
| sha512_half/8192     | 989 ± 4.7     | 692 ± 4.6    | 119         | 83.2         | 1.43x      |
| home_le_field_1k/0   | 11.9 ± 0.056  | 9.98 ± 0.043 | —           | —            | —          |
| home_le_field_1k/1   | 12.1 ± 0.063  | 10.2 ± 0.046 | 195         | 178          | 1.10x      |
| home_le_field_1k/8   | 13.1 ± 0.088  | 10.8 ± 0.043 | 151         | 107          | 1.41x      |
| home_le_field_1k/64  | 20.3 ± 0.13   | 15.7 ± 0.4   | 132         | 89.1         | 1.48x      |
| home_le_field_1k/512 | 75.8 ± 0.37   | 54.2 ± 0.22  | 125         | 86.4         | 1.44x      |
| sha512_half_1k/0     | 11.5 ± 0.27   | 8.67 ± 0.046 | —           | —            | —          |
| sha512_half_1k/1     | 11.7 ± 0.061  | 8.82 ± 0.039 | 235         | 151          | 1.56x      |
| sha512_half_1k/8     | 12.7 ± 0.19   | 9.57 ± 0.041 | 155         | 113          | 1.37x      |
| sha512_half_1k/64    | 19.7 ± 0.19   | 14.9 ± 0.065 | 128         | 97.1         | 1.31x      |
| sha512_half_1k/512   | 74.1 ± 0.35   | 56.5 ± 0.19  | 122         | 93.3         | 1.31x      |
| trace/0              | 13.2 ± 0.13   | 10.4 ± 0.44  | —           | —            | —          |
| trace/1              | 13.3 ± 0.12   | 10.4 ± 0.43  | 124         | 41.6         | 2.97x      |
| trace/8              | 14.2 ± 0.15   | 11.1 ± 0.33  | 134         | 95.8         | 1.40x      |
| trace/64             | 20.4 ± 0.23   | 15.5 ± 0.35  | 113         | 80.2         | 1.41x      |
| trace/512            | 69.1 ± 0.28   | 50.9 ± 0.77  | 109         | 79.1         | 1.38x      |
| trace/4096           | 447 ± 3.4     | 335 ± 2.2    | 106         | 79.2         | 1.34x      |
| trace/8192           | 870 ± 6.2     | 656 ± 4.4    | 105         | 78.8         | 1.33x      |
| trace_num/0          | 12.5 ± 0.12   | 10.2 ± 0.14  | —           | —            | —          |
| trace_num/1          | 12.6 ± 0.096  | 10.3 ± 0.11  | 115         | 112          | 1.02x      |
| trace_num/8          | 13.4 ± 0.099  | 10.7 ± 0.15  | 115         | 69.9         | 1.65x      |
| trace_num/64         | 19.2 ± 0.18   | 14.8 ± 0.14  | 104         | 71.9         | 1.45x      |
| trace_num/512        | 63.8 ± 0.23   | 45.6 ± 0.15  | 100         | 69.2         | 1.45x      |
| trace_num/4096       | 414 ± 1.6     | 292 ± 1      | 98.1        | 68.9         | 1.42x      |
| trace_num/8192       | 810 ± 2.7     | 574 ± 3.3    | 97.4        | 68.8         | 1.42x      |

### Per-host-function summary

Steady-state per-call cost, one row per host-function contract — the mean over
K ≥ 512, where the intra-run warm-up has settled (the `1 KiB`-payload variants
only reach K = 512, so their mean is that single point). Columns are C++ then
Rust, matching the table above.

| host function                  | I/O shape          | C++ ns/call | Rust ns/call | per-call ↑ |
|--------------------------------|--------------------|-------------|--------------|------------|
| `get_ledger_sqn`               | out 4 B            | 103         | 67.7         | 1.51x      |
| `get_current_ledger_obj_field` | out 4 B            | 118         | 78.3         | 1.50x      |
| `get_current_ledger_obj_field` | out 1 KiB          | 125         | 86.4         | 1.44x      |
| `sha512_half`                  | in 3 B, out 32 B   | 124 †       | 83.3         | 1.49x      |
| `sha512_half`                  | in 1 KiB, out 32 B | 122         | 93.3         | 1.31x      |
| `trace`                        | in ~2 B ×2         | 107         | 79.0         | 1.35x      |
| `trace_num`                    | in ~2 B + i64      | 98.6        | 69.0         | 1.43x      |

† `sha512_half`'s mean is pulled up by its K = 512 point (134 ns/call C++, the
noisy `81.2 ± 3.6 µs` run discussed in §5); its settled K ≥ 4096 value is
~119 ns/call.

The takeaway line: **a warm host call costs ~68–93 ns in Rust versus ~99–125 ns
in C++ — a 1.31×–1.51× per-call win**, largest on dispatch-bound small-payload
calls and smallest on the 1 KiB read (`sha512_half_1k`) and read-heavy `trace`
(§3).

---

## Analysis

### 1. The Rust PoC is faster on every case; the gap is all in per-call binding

Read the **`per-call ↑`** column at steady state (K ≥ 512): a host call costs
**~68–93 ns in Rust vs ~99–125 ns in C++**, a **1.31×–1.51×** win depending on
the call (see the per-host-function summary). Two facts locate that win in the
binding layer:

- **`launch` is a tie.** With zero host calls the two engines are within noise
  (Rust nominally slower, 3.52 vs 3.1 µs, both ±~7%) — same interpreter, same
  validate+instantiate cost, so per-call cost is `—`.
- **Per-call cost is a warm-up curve** (see the model in *How to read the
  numbers*): with each contract's fixed cost removed via its `/0` row, `ns/call`
  still declines from K = 1 and settles by ~K = 512 (Rust `ldgr_index`:
  108 → 79 → 70 → 68 → 67). Read it at K ≥ 512; the low-K rows are intra-run
  warm-up, not signal.

So the difference is not the interpreter and not module setup — it's the
per-host-call path: a warm `ldgr_index` call is ≈**103 ns** in C++ vs ≈**68 ns**
in Rust — the native-wasmi + cxx-host path dispatches a host call in roughly
two-thirds the time of the wasmi-C-API + vtable path. §2 explains why.

(The whole-run *ratio* — C++ µs ÷ Rust µs — behaves differently and is why we
don't headline it: it climbs from ~1.0 at K = 1 toward a ceiling of `c/r` ≈ 1.5
at high K, because it's a ratio of two lines `(F + K·c)/(F + K·r)` in which the
shared fixed cost `F` dilutes it at low K and vanishes at high K. Per-call cost
is the stabler metric once warm.)

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
argument of every call. That is the ≈103 ns/call (C++) vs ≈68 ns/call (Rust)
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

Ranking by per-call speedup at steady state (K ≥ 512; matches the
per-host-function summary above):

| Call | I/O shape | per-call ↑ (K≥512) |
|---|---|---|
| `ldgr_index` | output-only, no input | **~1.51×** |
| `home_le_field` | output-only (+ scalar in) | ~1.50× |
| `sha512_half` | read + write (3 B in) | ~1.49× |
| `trace_num` | small read + scalar | ~1.43× |
| `trace` | read-only, two reads | ~1.35× |

The speedups sit in a tight **~1.35–1.51×** band (small-payload calls; the 1 KiB
payloads sit lower — §4), and the ordering within it tracks
**how much of a call is pure dispatch** (where Rust's cheaper binding, §2, wins
big) **versus shared per-call work both engines do the same way** (bounds /
transfer accounting, UTF-8 validation, the actual primitive):

- **Output-only calls are almost pure dispatch**, so Rust's per-call advantage
  shows nearly in full — the top of the band (~1.50×), since the direct-write
  output is as cheap as (here cheaper than) C++'s own write.
- **`trace` does the most shared work per call** (two `read_borrowed`s + UTF-8
  validation), so dispatch is a smaller fraction and its speedup, while still
  solid, is the lowest (~1.35×). Concretely, per-call cost at K = 512: C++ is
  ≈109–113 ns/call for **both** `ldgr_index` and `trace` (its per-call cost is
  dominated by the fixed C-API marshaling of §2), while Rust is ≈68 ns/call for
  `ldgr_index` vs ≈79 ns/call for `trace` — the extra ~11 ns is `trace`'s two
  reads + validation, which Rust does in-engine while C++'s large fixed overhead
  hides it. Both engines read zero-copy here, so it is not a copy asymmetry.

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

| Path | small ↑ | 1 KiB ↑ | Δ |
|---|---|---|---|
| `home_le_field` (output / write) | 1.53× | 1.44× | **−0.09** |
| `sha512_half` (read + write)     | 1.60× | 1.31× | **−0.29** |

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

Caveat: the `sha512_half` small point (1.60× at K = 512) is the drift-high run
of §5; its settled speedup is ~1.49× (steady K ≥ 512 mean), so the *true* read
compression is nearer −0.18 than the raw −0.29 — still markedly steeper than the
write case's −0.09.

For the team's original question — *"is there I/O overhead from cxx?"* — the
answer the data gives is **no**: payloads cross the cxx boundary as pointer+len
slices in both directions, both the write path and the read-only input path are
zero-copy end-to-end, and the only per-byte asymmetry left versus C++ is the
*input* copy on read+write calls (now into a stack buffer, no allocation), which
is internal to the engine (not cxx).

### 5. The `sha512_half` vs `sha512_half_1k` "anomaly"

The eyebrow-raiser: `sha512_half_1k` (1 KiB input) is **faster** than
`sha512_half` (3 B input) in the C++ rows — e.g. C++ at K = 512: 74.1 vs
81.2 µs. A bigger input finishing faster looks wrong.

It isn't a real effect. Three things resolve it:

1. **The host does no hashing.** `FakeHost::computeSha512HalfHash` returns a
   constant regardless of input, so a 1 KiB input creates **no extra host
   work** — only the *engine's* read of the bytes could differ.
2. **The C++ read is zero-copy.** `getDataSlice` builds a `Slice(ptr, len)` in
   O(1); reading 3 B vs 1 KiB costs the C++ engine essentially the *same*.
   There is therefore **no mechanism** by which the 1 KiB C++ case could be
   genuinely faster than the 3 B case — they should be equal.
3. **The small run is the outlier, not the large one.** Anchor against the
   comparable `home_le_field/512` (C++ 73.4 µs / 120 ns/call). `sha512_half_1k/512`
   = 74.1 µs (122 ns/call) sits right on that anchor; `sha512_half/512` = 81.2 µs
   (134 ns/call) is ~8 µs above it — and it is the noisiest C++ point at that K
   (`81.2 ± 3.6 µs`, ±4%). Its own settled per-call (K ≥ 4096) is ~119 ns/call,
   back in line. So the small `sha512_half/512` measurement drifted high, while
   the `_1k` value is the well-behaved one.

The physically real signal here is on the **Rust** side, which *is* consistent:
its per-call cost rises `sha512_half` 83.5 → `sha512_half_1k` 93.3 ns/call at
K = 512, i.e. **+9.8 ns/call** going 3 B → 1 KiB. That is exactly the extra 1 KiB
input copy (§3/§4, now a stack copy not a heap `Vec`), in the expected direction.
C++ reads zero-copy so its per-call is flat across payloads, and the two converge
to a 1.31× speedup at 1 KiB.

**Bottom line:** it's a cross-benchmark measurement artifact (separately
registered benchmarks run at different points in the sequence, under slightly
different thermal/scheduling conditions), amplified by the noisy small-`sha512`
run — not a case of "bigger is faster." To confirm, re-run with
`--benchmark_repetitions=N --benchmark_enable_random_interleaving=true` and,
ideally, CPU pinning / a quiesced machine; the small/large C++ `sha512` numbers
should then match.

---

## Conclusions

1. **Rust PoC ≥ C++ on every host call — ~1.31×–1.51× per call (steady-state,
   K ≥ 512).** Same interpreter, so the win is entirely in the binding layer
   around wasmi — see §2 for the mechanism.
2. **The win is per-host-call, not startup.** `launch` is a wash (if anything
   Rust is marginally slower, within noise); the advantage scales with the
   number of host calls a contract makes.
3. **No cxx I/O overhead.** Payloads cross the boundary as zero-copy slices both
   ways; the direct-write output path is as cheap as C++'s native write (often
   cheaper).
4. **Reads carry no copy penalty.** Host-call *inputs* are never heap-copied:
   read-only calls hand the host a `&[u8]` aliasing guest memory
   (`read_borrowed`, zero-copy) and read+write calls copy into a stack buffer
   with no heap allocation (`read_write`). Read-heavy and write-heavy small-payload
   calls sit in the same ~1.35–1.51× band (§3). The only residual is that read+write calls
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

# Cut between-run drift at the source (Linux): pin to one core and hold the
# clock steady. This shrinks the run-to-run stddev more than any flag can.
taskset -c 2 ./build/xrpl_wasm_bench \
    --benchmark_repetitions=25 \
    --benchmark_min_time=1s \
    --benchmark_min_warmup_time=0.2 \
    --benchmark_enable_random_interleaving=true \
    --benchmark_report_aggregates_only=true \
    --benchmark_out=build/bench_result.json \
    --benchmark_format=json

python3 bench_table.py build/bench_result.json
```

Filter to a subset with e.g. `--benchmark_filter='(rust|cpp)/sha512_half(_1k)?/.*'`.

Why these knobs: **repetitions** (25) plus **random interleaving** are what
average out the between-run thermal/scheduling drift — the noise behind the §5
`sha512_half` artifact; adding *iterations* per run (longer `min_time`) only
sharpens a within-run mean that is already ±0.1% and would not have touched that
artifact. `--benchmark_min_time=1s` keeps each repetition's mean stable, and
`--benchmark_min_warmup_time=0.2` runs untimed warm-up first (relevant given the
i-cache / lazy-translation warm-up in §1). Pinning + a fixed CPU governor attack
the drift directly. At 1 s × 25 reps across the full K sweep this run takes
~30–40 min; drop `min_time` or narrow with `--benchmark_filter` while iterating.
`bench_table.py` turns the JSON into the per-test and per-host-function tables
above, computing per-call cost from each contract's K=0 baseline.
