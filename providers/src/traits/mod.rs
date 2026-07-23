//! 能力 trait 系统 — 编译期能力检查 + 泛型客户端。

pub mod capability;
pub mod client;
pub mod models;

pub use capability::*;
pub use client::*;
pub use models::*;
