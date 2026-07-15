//! 会话：MemoryManager（仍属 memory）+ SessionStore（实现在 `session` crate）。

pub mod manager;

pub use ::session::message_db;
pub use ::session::store;
