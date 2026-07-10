use crate::imports::register_host_functions;
use host_functions::HostFunctions;
use wasmi::{Config, Engine, Linker, Module, Store};

/// State threaded through every host call, stored in the wasmi [`Store`].
pub struct VmState {
    pub(crate) host: Box<dyn HostFunctions>,
}

/// Outcome of running an escrow contract to completion.
pub struct RunOutcome {
    /// The value returned by the exported entry point (`finish`): `> 0` means
    /// allow the escrow to finish.
    pub result: i32,
    /// Fuel (gas) consumed by the whole invocation — guest instructions plus
    /// the per-call host charges.
    pub fuel_used: u64,
}

/// Build the wasmi engine with the sandboxing knobs the escrow VM requires.
/// (Unchanged from the original skeleton: a deterministic, minimal-feature
/// configuration with fuel metering on.)
pub fn build_wasm_engine() -> Engine {
    let mut config = Config::default();
    config.consume_fuel(true);
    config.ignore_custom_sections(true);
    config.wasm_mutable_global(false);
    config.wasm_multi_value(false);
    config.wasm_sign_extension(false);
    config.wasm_saturating_float_to_int(false);
    config.wasm_bulk_memory(false);
    config.wasm_reference_types(false);
    config.wasm_tail_call(false);
    config.wasm_extended_const(false);
    config.floats(false);
    config.wasm_multi_memory(false);
    config.wasm_custom_page_sizes(false);
    config.wasm_memory64(false);
    config.wasm_wide_arithmetic(false);
    Engine::new(&config)
}

/// Run an escrow contract: compile `wasm`, give it `gas` fuel, service its host
/// calls through `host`, and call the exported `function_name` (`finish`).
///
/// This is the coarse, once-per-finish entry the C++ side will call across cxx
/// in Step 3.
pub fn run_escrow(
    wasm: &[u8],
    gas: u64,
    host: Box<dyn HostFunctions>,
    function_name: &str,
) -> Result<RunOutcome, String> {
    let engine = build_wasm_engine();
    let module = Module::new(&engine, wasm).map_err(|e| format!("compile: {e}"))?;

    let mut store = Store::new(&engine, VmState { host });
    store.set_fuel(gas).map_err(|e| format!("set_fuel: {e}"))?;

    let mut linker = Linker::<VmState>::new(&engine);
    register_host_functions(&mut linker)?;

    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| format!("instantiate: {e}"))?;
    let finish = instance
        .get_typed_func::<(), i32>(&store, function_name)
        .map_err(|e| format!("no entry point '{function_name}': {e}"))?;

    let result = finish
        .call(&mut store, ())
        .map_err(|e| format!("trap: {e}"))?;

    let remaining = store.get_fuel().unwrap_or(0);
    Ok(RunOutcome {
        result,
        fuel_used: gas.saturating_sub(remaining),
    })
}
