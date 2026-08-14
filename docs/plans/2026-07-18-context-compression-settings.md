# 上下文压缩完整配置 Implementation Plan

> 实现状态：已落地（2026-07-18）。

**Goal:** 偏好设置可查看并修改完整上下文卫生参数；写入 `config.yaml` 的 `compression:` 段后，Agent / 会话压实真正按配置生效。

**Architecture:** `memory::CompressionConfig` 持久化 → Tauri get/set/reset → Preferences 卡片；Agent 热读构造 `ToolCompressionManager` 与 mid-run / Gateway / recommend；`compact_chat_session` 读 `keep_tail_bubbles`。

## 落地文件

- `crates/agent-memory/src/config.rs` — `CompressionConfig` + load/set/reset
- `apps/desktop/src-tauri/src/compression_settings_commands.rs` — DTO / 校验 / 命令
- `crates/agent-core/src/compression.rs` — `from_config` / thrashing 参数化
- `crates/agent-core/src/runtime/mod.rs` / `mid_run_summary.rs` / `multi_turn.rs` / `context_usage.rs`
- `apps/desktop/src/components/settings/CompressionSettingsCard.tsx` + Preferences 接入
- `docs/context-compression.md` — 配置表更新
