// 生产代码禁止 `unwrap`（panic 会打断运行中的工作流）；测试里 `unwrap` 就是失败信号，
// 按仓库惯例放行，否则 `cargo clippy --all-targets` 会被上百条测试代码的 unwrap 打断。
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod engine;
pub mod error;
pub mod model;
pub mod nodes;
pub mod run_db;
pub mod store;
