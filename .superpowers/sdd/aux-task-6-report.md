# Aux Task 6 Report: 入梦与记忆审查凭据/降级

## Scope
- Modify: `frontend/src-tauri/src/dreaming_commands.rs`
- Modify: `agent/src/exec/memory_review.rs`
- Touch: `agent/src/control/smart_approval.rs`（清理 Task 5 遗留 unused import）

未改 `agent/src/runtime/mod.rs`：Task 2 已提供 `auxiliary_targets()`，本任务只需消费。

## What changed
- 入梦删除 `find_provider_by_backend(...).unwrap_or(ui)`；改走 `resolve_auxiliary_targets(Dreaming)`，按 preferred→fallback 重试。
- Background review job 改为携带完整 `Vec<ChatTarget>`（来自 `AuxiliaryTask::BackgroundReview`）；不再把 session api_key/base_url 硬套到显式 backend。
- 两次失败只记 review failure，不影响 Chat Done。

## Verification
- `cargo test -p agent exec::memory_review -- --nocapture` PASS（5）
- `cargo test -p agent -- --nocapture` PASS
- `cargo check -p astro-agent` PASS

## Self-review
- 显式目标使用自身 endpoint/key/model；失败后才试 primary fallback。
- 空 targets / 关闭 `background_review_enabled` 时跳过，不 panic。
