# Aux Task 2 Report: 设置 API、解析与目标透传

## Scope
- Create: `common/src/auxiliary_target.rs`
- Create: `frontend/src-tauri/src/auxiliary_commands.rs`
- Create: `frontend/src-tauri/src/auxiliary_resolver.rs`
- Modify: `common/src/lib.rs`, `proto/proto/astro.proto`
- Modify: `frontend/src-tauri/src/{lib,commands}.rs`, `frontend/src/types.ts`
- Modify: `agent/src/runtime/mod.rs`
- Modify: `backend/src/grpc/astro_service.rs`（计划 Step 4 要求；Files 列表漏写）

未做：设置面板 UI（Task 3）、各调用点改造（Tasks 4–7）。

## What changed
- Tauri：`get/set/reset` 辅助路由命令；`auto/auto` 或双显式校验；unavailable 仅看 enabled/hasApiKey。
- Resolver：显式 UI Provider ID → preferred + 可选 primary fallback；不可用时静默退 primary。
- Proto `AuxiliaryModelTarget` + `ChatRequest.auxiliary_targets`；`start_chat` 每轮解析五类下传。
- Backend 分组排序后 `AgentLoop::set_auxiliary_targets`；未下传任务回退主 `ChatTarget`。
- TS：`AuxiliaryTaskId` / `AuxiliaryTaskDto` / `AuxiliarySettingsDto`。

## Verification
- `cargo check -p astro-agent` PASS（`resolve_auxiliary_targets` 暂 unused，供后续任务）
- `cargo test -p common auxiliary` / `-p agent auxiliary_targets` / `-p backend parse_auxiliary_targets`
- `cargo test -p astro-agent --lib auxiliary_` → 15 passed

## Self-review
- API key 仅内存透传，不落盘。
- 未知 proto `task` / 单任务解析失败不阻塞主聊。
- 设置命令保存 UI Provider ID，与 resolver 约定一致。
