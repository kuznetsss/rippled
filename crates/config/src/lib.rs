pub mod cli_flags;
pub mod config_builder;
pub mod detect;
pub mod error;
pub mod ffi;
pub mod ini;
pub mod loader;
pub mod schema;

pub use crate::cli_flags::CliFlags;
pub use crate::config_builder::ConfigBuilder;
pub use crate::detect::detect_config_path_from_env;
pub use crate::error::ParseError;
pub use crate::loader::{ConfigFormat, IniWarnings, parse_from_file, parse_from_str};
pub use crate::schema::Config;
