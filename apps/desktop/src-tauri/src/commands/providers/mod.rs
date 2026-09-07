pub mod config;
pub mod core;
pub mod model_catalog;
pub mod openrouter_rankings;

// Re-export core items so `crate::commands::providers::X` still works.
pub use core::*;
