# 核心业务中文注释 Implementation Plan

> **For agentic workers:** 按 crate 分批为 Rust 源码补充 `//!` / `///` 中文注释；不改逻辑。

**Goal:** 为 agent / memory / providers / tools 全面补充中文 rustdoc 风格注释。

**Architecture:** 按已批准规范，逐 crate、逐文件只增改注释。

**Tech Stack:** Rust `//!` / `///`

---

### Task 1: agent crate

- [x] `src/lib.rs` 及各 `src/*.rs` 模块与公开/私有项注释
- [ ] 测试文件仅模块级 `//!`（可选简短）

### Task 2: memory crate

- [x] 同上，覆盖 workspace / DB / cron / dreaming 等

### Task 3: providers crate

- [x] trait、registry、各供应商适配、流式 HTTP

### Task 4: tools crate

- [x] registry、catalog、各工具实现（含 builtins 缺口补全）

### Task 5: 扩展业务 crate

- [x] 核心 crate 测试文件模块级 `//!`
- [x] `common` / `permissions` / `skills` / `mcp` / `backend` 中文注释

### Task 6: 剩余选项

- [x] `proto`（`astro.proto` + build/lib）
- [x] `backend` `astro_service.rs` 各 RPC 方法
- [x] `apps/desktop/src-tauri` 模块与命令注释
- [x] `apps/desktop/src` 组件 Props + 大面板私有辅助函数中文 JSDoc

### 验证

- [x] `cargo check` 相关 crate 通过
- [x] 确认无逻辑 diff（仅注释）
- [x] `frontend` `npx tsc -b` 通过
