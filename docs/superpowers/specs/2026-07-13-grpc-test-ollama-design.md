# gRPC 测试去 Ollama 硬依赖

**日期:** 2026-07-13  
**状态:** 已实现  
**范围:** `backend/tests/grpc_test.rs` 仅

## 目标

默认 `cargo test -p backend` 不依赖本机 Ollama / 已拉取模型。

## 做法

1. **默认测** `test_grpc_connect_and_query_memory`：起 gRPC → `query_memory`；不发 LLM chat。
2. **Live 测** `test_grpc_chat_ollama_live`：仅当 `ASTRO_LIVE_OLLAMA=1` 时跑 ollama chat；否则立刻 return。

## 非目标

注入 mock provider、改 `AstroServiceImpl` 生产路径。
