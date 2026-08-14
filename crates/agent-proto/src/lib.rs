//! Astro gRPC 协议定义（由 `astro.proto` 经 tonic 生成）。
//!
//! 本 crate 在 `build.rs` 中编译 `proto/astro.proto`，同时生成 server 与 client。
//! 下游通过 `proto::astro_service_server::AstroService` / `astro_service_client`
//! 以及各 message 类型使用生成代码。

#![allow(clippy::all)]
tonic::include_proto!("astro");
