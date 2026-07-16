# Aux Task 4 Report: 压缩辅助路由与降级

## Scope
- Modify: `frontend/src-tauri/src/compaction_commands.rs`

未做：智能审批 / 入梦 / 审查 / 标题生成调用点改造。

## What changed
- 用当前激活 UI provider 构造 primary `ChatTarget`，再经 `resolve_auxiliary_targets(Compaction)` 解析 preferred + fallback。
- 抽取 `next_compaction_target` / `summarize_with_target` / `summarize_with_targets`，按顺序尝试；全部失败才回退启发式摘要并保持 `degraded = true`。
- 补充纯函数测试，覆盖 preferred→fallback→None 顺序。

## Verification
- `cargo test -p astro-agent compaction -- --nocapture` PASS
- `cargo check -p astro-agent` PASS

## Self-review
- 压缩仍是 Tauri 侧直接调用，不依赖 AgentLoop 已注入目标；与本任务「resolve_auxiliary_targets」约定一致。
- 显式辅助模型失败后会再试 primary fallback；auto 路由无 fallback，失败即进启发式。
