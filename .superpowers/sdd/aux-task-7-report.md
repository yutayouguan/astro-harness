# Aux Task 7 Report: 首轮异步标题与元数据事件

## Scope
- Create: `common/src/title.rs`, `agent/src/exec/title_generation.rs`
- Modify: `agent/src/exec/mod.rs`, `common/src/lib.rs`
- Modify: `proto/proto/astro.proto`, `backend/src/{session_events,lib,grpc/astro_service}.rs`
- Modify: `frontend/src-tauri/src/{commands,lib,session_events,dreaming_commands,memory_commands}.rs`
- Modify: `frontend/src/components/chat/ChatSessionList.tsx`, `frontend/src/i18n/messages.ts`

## What changed
- `sanitize_title`：去 Markdown/引号包装并截断 40 Unicode chars。
- Done 后 `spawn_title_generation_after_turn`：空标题 + 完整首轮才生成；`set_session_title_if_empty` 不覆盖手动标题。
- Proto / hub / Tauri 增加 `session_metadata_changed`；侧栏监听后更新 summary。
- Tauri `regenerate_session_title` 强制覆盖，并启用侧栏「重新生成标题」。

## Verification
- `cargo test -p common title` PASS
- `cargo test -p agent exec::title_generation` PASS
- `cargo test -p backend session_events` PASS
- `cargo test -p session` PASS
- `cargo check -p astro-agent` PASS
- `cd frontend && npm run build` PASS

## Self-review
- 自动标题与手动重命名并发时，条件写入保证手动优先。
- 重新生成走强制 `set_session_title`，与自动路径语义分离。
