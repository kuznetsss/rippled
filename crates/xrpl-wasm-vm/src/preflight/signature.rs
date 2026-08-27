//! The wasm function type an import must carry, and the words a refusal names it in.
//!
//! One module decides both, so what a contract author is told cannot drift from what
//! was compared: [`fault`] renders the same [`declared_func_type`] that [`matches`]
//! compares against, and renders the import's type through it too, so the two shapes
//! in the message are spelled by one function.

use wasmi::{FuncType, ValType};
use xrpl_host_functions::{HostFunctionSpec, WasmValType};

/// Whether `imported` is the type the ABI declares for `function`.
pub(super) fn matches(imported: &FuncType, function: HostFunctionSpec) -> bool {
    *imported == declared_func_type(function)
}

/// Why `imported` is not what `function` is declared with, for the caller to prefix
/// with the import's name. Both shapes, so a contract author sees the difference
/// rather than being told only that there is one.
///
/// Allocates, so it is built only once [`matches`] has already failed.
pub(super) fn fault(imported: &FuncType, function: HostFunctionSpec) -> String {
    format!(
        "has the wrong signature, expected '{}', found '{}'",
        func_type_to_string(&declared_func_type(function)),
        func_type_to_string(imported)
    )
}

/// The type the ABI declares for `function`, in this engine's vocabulary.
///
/// No allocation for any declaration in this ABI: wasmi keeps a function type's value
/// types inline up to 14 of them (`FuncTypeInner::INLINE_SIZE`, the 32-bit figure),
/// and the widest host function is eight parameters and one result.
pub(super) fn declared_func_type(function: HostFunctionSpec) -> FuncType {
    FuncType::new(
        function.wasm_params().iter().copied().map(val_type),
        function.wasm_result().map(val_type),
    )
}

/// The ABI's value types as this engine spells them, and the one place the two
/// vocabularies meet: [`xrpl_host_functions`] is dependency-free so that the guest
/// stdlib can link it, so it cannot name [`ValType`] itself. `xrpl-wasm-vm-ffi`'s
/// `crossed` is the same shape, carrying a `TraceDataType` to the cxx bridge's own
/// enum.
fn val_type(ty: WasmValType) -> ValType {
    match ty {
        WasmValType::I32 => ValType::I32,
        WasmValType::I64 => ValType::I64,
    }
}

/// A function type as `'(i32, i32) -> i32'`, or `'(i32, i32) -> ()'` where there is no
/// result — the spelling [`super::entry_point_fault`] uses for the entry point's own
/// type, since a contract author reads both.
fn func_type_to_string(ty: &FuncType) -> String {
    let list = |types: &[ValType]| {
        types
            .iter()
            .map(|ty| val_type_to_str(*ty))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match ty.results() {
        [] => format!("({}) -> ()", list(ty.params())),
        [only] => format!("({}) -> {}", list(ty.params()), val_type_to_str(*only)),
        // Neither side can reach this: the ABI declares no multi-result function, and
        // `wasm_multi_value(false)` refuses a module that imports one at the compile
        // stage. Spelled rather than argued about, since the argument is elsewhere.
        results => format!("({}) -> ({})", list(ty.params()), list(results)),
    }
}

/// A value type as the wasm text format writes it, which is how a contract author
/// wrote it. `&str` rather than `String`: every spelling is a literal.
fn val_type_to_str(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "i32",
        ValType::I64 => "i64",
        ValType::F32 => "f32",
        ValType::F64 => "f64",
        ValType::V128 => "v128",
        ValType::FuncRef => "funcref",
        ValType::ExternRef => "externref",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spelling both refusals use, and the one a C++ caller logs. A single
    /// result is bare, as the entry point's `'() -> i32'` is; no result is `()`, so
    /// a `trace` fault says what the function does rather than that a list is empty.
    #[test]
    fn a_signature_is_spelled_as_the_text_format_writes_it() {
        let spelled = |params: &[ValType], results: &[ValType]| {
            func_type_to_string(&FuncType::new(params.to_vec(), results.to_vec()))
        };

        assert_eq!(spelled(&[], &[ValType::I32]), "() -> i32");
        assert_eq!(
            spelled(&[ValType::I32, ValType::I64], &[]),
            "(i32, i64) -> ()"
        );
        assert_eq!(spelled(&[ValType::F64], &[ValType::F32]), "(f64) -> f32");
    }

    /// The ABI's two value types cross to the engine's own, and to nothing else.
    /// Pinned rather than left to the arms reading right, for the reason
    /// `xrpl-wasm-vm-ffi`'s `every_data_type_crosses_as_the_same_wire_value` pins its
    /// crossing: an exhaustive `match` forces an arm per variant, not a correct one.
    #[test]
    fn the_abi_value_types_cross_as_themselves() {
        assert_eq!(val_type(WasmValType::I32), ValType::I32);
        assert_eq!(val_type(WasmValType::I64), ValType::I64);
    }
}
