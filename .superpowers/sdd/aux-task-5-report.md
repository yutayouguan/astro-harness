# Aux Task 5 Report: 智能审批辅助路由

## Scope
- Modify: `agent/src/control/smart_approval.rs`
- Modify: `agent/src/streaming/tools_exec.rs`

## What changed
- 新增 `ApprovalTarget`，evaluator 改为接受最多 2 个目标的序列。
- 抽出可测的 `evaluate_smart_approval_with_completion`：preferred 失败后试 fallback；两者都失败返回 Err（调用方保留 Ask，不自动 allow）。
- `tools_exec` 读取 AgentLoop 已注入的 `AuxiliaryTask::SmartApproval` 目标链；保留 `ASTRO_SMART_APPROVAL` 总开关。
- agent 不读 keyring，也不依赖 Tauri。

## Verification
- `cargo test -p agent control::smart_approval -- --nocapture` PASS（6）
- `cargo test -p agent -- --nocapture` PASS（103）

## Self-review
- 超时与全部失败仍回退 Ask，不放行危险命令。
- 目标为空时直接 Ask，兼容旧客户端未下传辅助目标的情况（`auxiliary_targets()` 仍会回退主 ChatTarget，通常非空）。
