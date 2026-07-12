mod catalog;
pub mod templates;
mod validate;

pub use catalog::ASTRO_CATALOG_ID;
pub use validate::{validate_operations, Error};
