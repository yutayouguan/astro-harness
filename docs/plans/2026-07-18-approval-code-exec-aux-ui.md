# Approval, Code Execution Security, and Auxiliary UI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 锁定危险命令审批语义、限制 `code_exec` 资源与凭据暴露，并让辅助模型面板明确显示实际生效路由。

**Architecture:** 审批分流抽成可测试的纯策略，串流执行只负责 HITL/辅模型副作用；`code_exec` 在子进程启动前清理环境并在 Unix 设置 rlimit；辅助模型 UI 基于 provider state 解析自动路由的真实 provider/model，并给出成本提示。

**Tech Stack:** Rust、Tokio、libc、Tauri、React、TypeScript。

---

### Task 1: 审批策略集成测试

**Files:**
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`

1. 抽取纯函数，输入 `command`、`ApprovalMode`、白名单，输出 hardline/allowlist/off/manual/smart 分支策略。
2. 编写覆盖 hardline、off、manual、白名单的单元测试。
3. 运行 `cargo test -p agent tools_exec`，确认通过。

### Task 2: code_exec 子进程安全护栏

**Files:**
- Modify: `crates/agent-tools/src/builtin/shell/code_exec.rs`
- Modify: `docs/coding-tools.md`

1. 先写敏感环境变量判定与环境过滤测试。
2. 默认 `env_clear()`，仅传递解释器运行所需的安全环境变量；剥离 `KEY/TOKEN/SECRET/PASSWORD/CREDENTIAL/AUTH` 等名称。
3. Unix `pre_exec` 设置 CPU、地址空间、文件大小、进程数与打开文件数上限。
4. 修复超时分支的临时文件清理，并让描述明确这是资源护栏而非硬沙箱。
5. 运行 `cargo test -p tools --lib code_exec`。

### Task 3: 辅助模型实际生效路由提示

**Files:**
- Modify: `apps/desktop/src/components/settings/AuxiliaryModelsPanel.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/styles/features/preferences.css`

1. 从 `providersState.active_provider_id` 与 provider 默认模型解析自动路由的实际 provider/model。
2. 自动项显示“继承：provider / model”，不再只显示“跟随主模型”。
3. 对 background review 标出默认关闭/成本提示，对辅助路由使用主模型时显示轻量成本提醒。
4. 运行 `npx tsc --noEmit`。

### Task 4: 全量验证与提交

1. 运行相关 Rust 测试与 `cargo clippy`。
2. 运行前端 TypeScript 类型检查并读取变更文件 lint。
3. 只暂存本次相关文件，创建一个说明 why 的 commit。
