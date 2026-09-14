//! **Throwaway.** Where a run's time goes, and what the linker's share of it is.
//!
//! ```text
//! cargo test --release -p xrpl-wasm-vm --lib -- --ignored --nocapture
//! ```
//!
//! Not tests: nothing asserts, and a number moving is not a failure. In `src` rather
//! than `tests` because what is being measured is crate-private — the two lines
//! `Linker::new` and `register_host_functions`, and the `Stopwatch` in `vm.rs` that
//! times `run`'s stages from the inside.
//!
//! Every number is the mean of [`RUNS`] timed samples, `±` the sample standard
//! deviation — **the spread of the samples, not the error on the mean**, which is
//! smaller by √n. A row near the printed clock floor says only "small": one clock
//! read and one push delimit each stage, two clock reads each total.
//!
//! **`min` is there because one stage drifts.** `Module::new` leaks into the
//! process-global engine — a few hundred bytes for a small module, far more for a
//! large one, and the largest case here reaches ~2.4 GB of RSS over [`RUNS`] runs —
//! so `compile` gets slower as a batch proceeds and its mean carries that. A mean
//! far above the min is a drifting stage, not a noisy one.

use std::fmt;
use std::hint::black_box;
use std::time::{Duration, Instant};

use wasmi::Linker;
use xrpl_host_functions::{HostFunctionSpec, WasmValType};

use crate::register::{Bodies, register_host_functions};
use crate::support::{Answer, ENTRY, FakeHost, ONE_PAGE, PLENTY_OF_GAS, assemble, module};
use crate::vm::{VmState, timing, wasm_engine};

/// Timed samples behind every number reported.
const RUNS: usize = 5_000;

/// Samples taken before any is kept, so no number carries a cold allocator or a
/// cold branch predictor.
const WARMUP: usize = 500;

/// A measured time in microseconds.
#[derive(Clone, Copy)]
struct Measured {
    mean: f64,
    stddev: f64,
    min: f64,
}

impl Measured {
    fn of(samples: &[Duration]) -> Measured {
        let n = f64::from(u32::try_from(samples.len()).expect("the sample count fits in a u32"));
        let mean = samples.iter().map(|s| s.as_secs_f64()).sum::<f64>() / n;
        // The sample standard deviation: these are a sample of this machine's
        // behaviour, not its population.
        let variance = samples
            .iter()
            .map(|s| (s.as_secs_f64() - mean).powi(2))
            .sum::<f64>()
            / (n - 1.0);
        let min = samples
            .iter()
            .map(|s| s.as_secs_f64())
            .fold(f64::INFINITY, f64::min);

        Measured {
            mean: mean * 1e6,
            stddev: variance.sqrt() * 1e6,
            min: min * 1e6,
        }
    }
}

impl fmt::Display for Measured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:>9.3} ± {:>7.3} us   min {:>8.3}",
            self.mean, self.stddev, self.min
        )
    }
}

/// [`RUNS`] samples of `body`, timed one at a time.
fn measure<T>(mut body: impl FnMut() -> T) -> Measured {
    for _ in 0..WARMUP {
        black_box(body());
    }

    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let started = Instant::now();
        black_box(body());
        samples.push(started.elapsed());
    }
    Measured::of(&samples)
}

/// What the two lines under analysis cost, and what one `func_wrap` of that costs.
#[test]
#[ignore = "a measurement, not an assertion"]
fn what_building_the_linker_costs() {
    let engine = wasm_engine();
    let count = HostFunctionSpec::ALL.len();

    let floor = measure(|| ());
    let empty = measure(|| Linker::<VmState<'_>>::new(engine));
    let registered = measure(|| {
        let mut linker = Linker::<VmState<'_>>::new(engine);
        register_host_functions::<Bodies>(&mut linker).expect("the ABI must register");
        linker
    });
    let clone = {
        let mut linker = Linker::<VmState<'_>>::new(engine);
        register_host_functions::<Bodies>(&mut linker).expect("the ABI must register");
        measure(|| linker.clone())
    };

    println!("\nbuilding the linker — construction and drop, as a run pays for it");
    println!("  measuring nothing          {floor}   (the clock floor)");
    println!("  Linker::new                {empty}");
    println!("  new + {count} func_wrap        {registered}");
    println!(
        "  per host function          {:>9.3} us                             \
         (the difference of the two above)",
        (registered.mean - empty.mean) / f64::from(u32::try_from(count).unwrap_or(1))
    );
    println!("  clone of a built linker    {clone}   (what a cached one would cost per run)");
}

/// The same numbers as a share of whole runs, which is the question the linker's
/// cost only means anything against.
#[test]
#[ignore = "a measurement, not an assertion"]
fn where_a_runs_time_goes() {
    let host = FakeHost::new().answering_sqn(Answer::bytes([1, 2, 3, 4]));

    for case in cases() {
        report(&case, &host);
    }
}

/// One module to run.
struct Case {
    name: String,
    wasm: Vec<u8>,
}

fn cases() -> Vec<Case> {
    let sqn = derived_import(HostFunctionSpec::GetLedgerSqn);
    let every_import: Vec<String> = HostFunctionSpec::ALL
        .iter()
        .copied()
        .map(derived_import)
        .collect();

    let mut cases = vec![
        case("no imports", &[ONE_PAGE.to_string()], "(i32.const 1)"),
        case(
            "1 import, 1 call",
            &[sqn.clone(), ONE_PAGE.to_string()],
            &calls(1),
        ),
        case(
            "1 import, 50 calls",
            &[sqn.clone(), ONE_PAGE.to_string()],
            &calls(50),
        ),
    ];

    // The ceiling for "register only what was imported": a contract that imports
    // the whole ABI has to end up no worse than it is today.
    let mut all = every_import;
    all.push(ONE_PAGE.to_string());
    cases.push(case(
        &format!("{} imports, 1 call", HostFunctionSpec::ALL.len()),
        &all,
        &calls(1),
    ));

    // Contract-sized bytes, so compilation weighs what it will in production.
    for functions in [128u32, 1_024] {
        let mut parts = vec![ONE_PAGE.to_string()];
        parts.extend(filler(functions));
        cases.push(case(
            &format!("{functions} filler functions"),
            &parts,
            "(i32.const 1)",
        ));
    }

    cases
}

fn case(name: &str, parts: &[String], body: &str) -> Case {
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    Case {
        name: name.to_string(),
        wasm: assemble(&module(&parts, body)),
    }
}

/// One `(import …)`, spelled from the ABI's derived signature and bound to the
/// wasm name so a body can call it.
fn derived_import(function: HostFunctionSpec) -> String {
    let spelled = |declared| match declared {
        WasmValType::I32 => "i32",
        WasmValType::I64 => "i64",
    };
    let types: Vec<&str> = function.wasm_params().iter().copied().map(spelled).collect();
    let params = match types.as_slice() {
        [] => String::new(),
        types => format!(" (param {})", types.join(" ")),
    };
    let result = match function.wasm_result() {
        Some(result) => format!(" (result {})", spelled(result)),
        None => String::new(),
    };
    let name = function.wasm_name();

    format!(r#"(import "host_lib" "{name}" (func ${name}{params}{result}))"#)
}

/// A body calling `ldgr_index` `n` times.
fn calls(n: u32) -> String {
    format!(
        "(local $i i32)
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (i32.const {n})))
        (drop (call $ldgr_index (i32.const 0) (i32.const 4)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (i32.const 1)"
    )
}

/// `n` functions nothing calls: module bytes to compile and translate.
fn filler(n: u32) -> Vec<String> {
    (0..n)
        .map(|i| {
            format!("(func $f{i} (param i32) (result i32) (i32.add (local.get 0) (i32.const {i})))")
        })
        .collect()
}

/// One case's stage breakdown, from [`RUNS`] runs of it.
///
/// The wall time and the stages come from the same runs, so the gap between them is
/// what falls outside the marks — the linker's and the store's frees, which happen
/// after the last one.
fn report(case: &Case, host: &FakeHost) {
    let once =
        || crate::run(&case.wasm, PLENTY_OF_GAS, host, ENTRY).expect("the module must run");

    for _ in 0..WARMUP {
        black_box(once());
    }

    let mut wall = Vec::with_capacity(RUNS);
    let mut totals = Vec::with_capacity(RUNS);
    let mut stages: Vec<(&'static str, Vec<Duration>)> = Vec::new();
    for _ in 0..RUNS {
        let started = Instant::now();
        black_box(once());
        wall.push(started.elapsed());

        let marked = timing::stages();
        totals.push(marked.iter().map(|&(_, elapsed)| elapsed).sum());
        if stages.is_empty() {
            stages = marked
                .iter()
                .map(|&(stage, _)| (stage, Vec::with_capacity(RUNS)))
                .collect();
        }
        for (samples, (stage, elapsed)) in stages.iter_mut().zip(marked) {
            assert_eq!(samples.0, stage, "a run marks its stages in one order");
            samples.1.push(elapsed);
        }
    }

    let measured = Measured::of(&totals);
    println!(
        "\n{} — {} module bytes, {RUNS} runs",
        case.name,
        case.wasm.len()
    );
    for (stage, samples) in stages {
        let stage_time = Measured::of(&samples);
        println!(
            "  {stage:<14} {stage_time}  {:>5.1}%",
            100.0 * stage_time.mean / measured.mean
        );
    }
    println!("  {:<14} {measured}", "stages");
    println!("  {:<14} {}", "run (wall)", Measured::of(&wall));
}
