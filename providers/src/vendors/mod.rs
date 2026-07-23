//! 旧供应商兼容层（逐步淘汰）。
//!
//! 新厂商实现在 [`crate::impls`]。
//! `ProfileBackedProvider` 仅用于 verify / image_gen 旧路径。

pub mod profile_backed;

pub mod azure;
pub mod claude;
