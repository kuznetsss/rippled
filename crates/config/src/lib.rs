#[cxx::bridge(namespace = "rs::config")]
mod ffi {
    extern "Rust" {
        type Config;
    }
}

//
pub struct Config {}
