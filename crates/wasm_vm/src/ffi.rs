//! cxx bridge for the wasm engine. Filled in in Step 3 (C++ -> Rust coarse
//! entry) and Step 4 (Rust -> C++ host-function forwarding).
#[cxx::bridge(namespace = "rs::wasm_vm")]
mod ffi {}
