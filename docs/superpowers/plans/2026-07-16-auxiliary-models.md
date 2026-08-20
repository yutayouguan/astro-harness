# Auxiliary Models Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为标题生成、上下文压缩、智能审批、入梦和记忆审查提供统一辅助模型路由、主模型降级和设置界面。

**Architecture:** `memory::AuxiliaryConfig` 保存五条稳定的 `auto/provider/model` 路由；Tauri 将 UI Provider ID 转换成运行时 Provider 与凭据；各调用点使用统一“显式模型一次、主模型一次”的执行策略。标题生成在首轮 Done 后异步运行，并通过会话变更事件刷新侧栏。

**Tech Stack:** Rust 2021、serde_yaml、Tauri 2、ProviderRegistry、React 18、TypeScript、现有 Provider 模型缓存

## Global Constraints

- 配置仅保存 Provider ID 与 Model ID，不复制 API key。
- `auto/auto` 跟随当前主模型。
- 显式模型不可用或调用失败时回退当前主模型一次。
- 标题与后台审查最终失败不得打断聊天。
- 压实与智能审批最终失败必须向前台报告并保持原状态。
- 网页抓取、技能搜索和 MCP 不增加模型配置。
- 保留 `ASTRO_SMART_APPROVAL` 作为智能审批总开关。
- 依赖 `2026-07-16-session-management.md` 提供的 `set_session_title_if_empty`、`first_turn_text` 与会话刷新事件。
- 只提交本计划相关文件，不包含现有 `crates/agent-tools/src/builtin/media/music_gen.rs` 改动。

---

## File Map

- Modify: `crates/agent-memory/src/config.rs` — 五类 route、YAML 写回与重置。
- Modify: `crates/agent-memory/src/lib.rs` — API re-export。
- Create: `apps/desktop/src-tauri/src/auxiliary_commands.rs` — 设置 DTO/commands。
- Create: `apps/desktop/src-tauri/src/auxiliary_resolver.rs` — UI Provider、凭据与 fallback 目标解析。
- Create: `common/src/auxiliary_target.rs` — 跨 Tauri/backend 的任务目标类型。
- Modify: `common/src/lib.rs` — 导出任务目标类型。
- Modify: `proto/proto/astro.proto` — 在 ChatRequest 透传已解析的辅助目标。
- Modify: `apps/desktop/src-tauri/src/commands.rs` — `start_chat` 注入辅助目标。
- Modify: `crates/agent-core/src/runtime/mod.rs` — AgentLoop 保存当前辅助目标。
- Modify: `apps/desktop/src-tauri/src/lib.rs` — 模块与 command 注册。
- Modify: `apps/desktop/src-tauri/src/compaction_commands.rs` — compaction 辅助路由。
- Modify: `apps/desktop/src-tauri/src/dreaming_commands.rs` — dreaming 统一解析与重试。
- Modify: `crates/agent-core/src/control/smart_approval.rs` — 可注入辅助调用目标。
- Modify: `crates/agent-core/src/streaming/tools_exec.rs` — smart approval 路由。
- Modify: `crates/agent-core/src/exec/memory_review.rs` — background review 凭据与回退。
- Create: `crates/agent-core/src/exec/title_generation.rs` — 标题清理和后台生成。
- Modify: `crates/agent-core/src/exec/mod.rs` — 标题模块导出。
- Modify: `crates/agent-server/src/grpc/astro_service.rs` — Done 后触发标题。
- Modify: `crates/agent-server/src/session_events.rs` and `proto/proto/astro.proto` — 会话元数据事件。
- Modify: `apps/desktop/src-tauri/src/session_events.rs` — 事件 DTO 转换。
- Create: `apps/desktop/src/hooks/settings/useAuxiliarySettings.ts` — 设置读写。
- Create: `apps/desktop/src/components/settings/AuxiliaryModelsPanel.tsx` — 五任务设置页。
- Modify: `apps/desktop/src/App.tsx` — 设置导航挂载。
- Modify: `apps/desktop/src/types.ts` — DTO。
- Modify: `apps/desktop/src/i18n/messages.ts` — 中英文文案。
- Modify: `docs/memory.md` — 配置文档。

---

### Task 1: 扩展五类辅助配置与 YAML 写回

**Files:**
- Modify: `crates/agent-memory/src/config.rs`
- Modify: `crates/agent-memory/src/lib.rs`

**Interfaces:**
- Produces: `AuxiliaryKind::{TitleGeneration, Compaction, SmartApproval, Dreaming, BackgroundReview}`
- Produces: `set_auxiliary_route(base, kind, route)`
- Produces: `reset_all_auxiliary_routes(base)`

- [x] **Step 1: 写失败测试**

在 `crates/agent-memory/src/config.rs` tests 增加：

```rust
#[test]
fn auxiliary_defaults_cover_all_five_tasks() {
    let cfg = AuxiliaryConfig::default();
    for kind in AuxiliaryKind::ALL {
        assert_eq!(cfg.route(kind), &AuxiliaryRoute::default());
    }
}

#[test]
fn set_route_preserves_unrelated_yaml() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.yaml"),
        "hooks:\n  enabled: true\nmemory:\n  write_approval: true\n",
    ).unwrap();
    set_auxiliary_route(
        dir.path(),
        AuxiliaryKind::Compaction,
        AuxiliaryRoute { provider: "provider-1".into(), model: "small".into() },
    ).unwrap();
    let text = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
    assert!(text.contains("enabled: true"));
    assert!(text.contains("write_approval: true"));
    assert_eq!(load_auxiliary_config(dir.path()).compaction.model, "small");
}

#[test]
fn reset_all_routes_keeps_background_review_enabled() {
    let dir = tempfile::tempdir().unwrap();
    set_background_review_enabled(dir.path(), true).unwrap();
    set_auxiliary_route(
        dir.path(),
        AuxiliaryKind::Dreaming,
        AuxiliaryRoute { provider: "p".into(), model: "m".into() },
    ).unwrap();
    let cfg = reset_all_auxiliary_routes(dir.path()).unwrap();
    assert!(cfg.background_review_enabled);
    assert_eq!(cfg.dreaming, AuxiliaryRoute::default());
}
```

- [x] **Step 2: 运行并确认失败**

Run: `cargo test -p memory config::tests -- --nocapture`  
Expected: compile FAIL，字段与 API 尚不存在。

- [x] **Step 3: 扩展类型与稳定 key**

```rust
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct AuxiliaryConfig {
    #[serde(default)]
    pub background_review_enabled: bool,
    #[serde(default)]
    pub title_generation: AuxiliaryRoute,
    #[serde(default)]
    pub compaction: AuxiliaryRoute,
    #[serde(default)]
    pub smart_approval: AuxiliaryRoute,
    #[serde(default)]
    pub dreaming: AuxiliaryRoute,
    #[serde(default)]
    pub background_review: AuxiliaryRoute,
}

impl AuxiliaryKind {
    pub const ALL: [Self; 5] = [
        Self::TitleGeneration,
        Self::Compaction,
        Self::SmartApproval,
        Self::Dreaming,
        Self::BackgroundReview,
    ];

    pub const fn config_key(self) -> &'static str {
        match self {
            Self::TitleGeneration => "title_generation",
            Self::Compaction => "compaction",
            Self::SmartApproval => "smart_approval",
            Self::Dreaming => "dreaming",
            Self::BackgroundReview => "background_review",
        }
    }
}

impl AuxiliaryConfig {
    pub fn route(&self, kind: AuxiliaryKind) -> &AuxiliaryRoute {
        match kind {
            AuxiliaryKind::TitleGeneration => &self.title_generation,
            AuxiliaryKind::Compaction => &self.compaction,
            AuxiliaryKind::SmartApproval => &self.smart_approval,
            AuxiliaryKind::Dreaming => &self.dreaming,
            AuxiliaryKind::BackgroundReview => &self.background_review,
        }
    }
}
```

让 `resolve_auxiliary` 调用 `aux.route(kind)`。

- [x] **Step 4: 实现 route 写回与全部重置**

`set_auxiliary_route` 在 `["auxiliary", kind.config_key()]` mapping 写入 `provider/model`；`reset_all_auxiliary_routes` 循环写五项 `auto/auto`，一次 load、一次原子 save，不能调用五次 save。

- [x] **Step 5: 运行 memory 测试**

Run: `cargo test -p memory -- --nocapture`  
Expected: PASS。

- [x] **Step 6: Commit**

```bash
git add memory/src/config.rs memory/src/lib.rs
git commit -m "feat(memory): configure five auxiliary model routes"
```

---

### Task 2: Tauri 辅助模型设置 API 与目标解析

**Files:**
- Create: `apps/desktop/src-tauri/src/auxiliary_commands.rs`
- Create: `apps/desktop/src-tauri/src/auxiliary_resolver.rs`
- Create: `common/src/auxiliary_target.rs`
- Modify: `common/src/lib.rs`
- Modify: `proto/proto/astro.proto`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `crates/agent-core/src/runtime/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/types.ts`

**Interfaces:**
- Produces commands: `get_auxiliary_settings`, `set_auxiliary_route`, `reset_auxiliary_route`, `reset_all_auxiliary_routes`
- Produces: `resolve_auxiliary_targets(kind, primary) -> AuxiliaryTargets`
- Produces: 每次 `start_chat` 向 AgentLoop 更新五类已解析目标。

- [x] **Step 1: 定义 DTO**

Rust 与 TypeScript 保持 camelCase：

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxiliaryTaskDto {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub display_label: String,
    pub unavailable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxiliarySettingsDto {
    pub tasks: Vec<AuxiliaryTaskDto>,
    pub active_provider_id: Option<String>,
    pub active_model: String,
}
```

```typescript
export type AuxiliaryTaskId =
  | "title_generation"
  | "compaction"
  | "smart_approval"
  | "dreaming"
  | "background_review";

export type AuxiliaryTaskDto = {
  id: AuxiliaryTaskId;
  provider: string;
  model: string;
  displayLabel: string;
  unavailable: boolean;
};
```

- [x] **Step 2: 实现 settings commands**

`task` 通过穷举 match 转成 `AuxiliaryKind`，未知值返回 `unknown auxiliary task`。`set_auxiliary_route` 只接受 `(auto, auto)` 或两个非空值，不允许半自动组合，避免 provider/model 来源不一致：

```rust
fn parse_route(provider: String, model: String) -> Result<AuxiliaryRoute, String> {
    let provider = provider.trim().to_string();
    let model = model.trim().to_string();
    let both_auto = provider.eq_ignore_ascii_case("auto")
        && model.eq_ignore_ascii_case("auto");
    let both_explicit = !provider.is_empty()
        && !model.is_empty()
        && !provider.eq_ignore_ascii_case("auto")
        && !model.eq_ignore_ascii_case("auto");
    if !both_auto && !both_explicit {
        return Err("provider and model must both be auto or explicit".into());
    }
    Ok(AuxiliaryRoute { provider, model })
}
```

显式 `provider` 统一保存 UI Provider ID。`get_auxiliary_settings` 用 `get_providers_state` 检查 provider enabled/hasApiKey，并生成展示标签；模型缓存缺失不判 unavailable，避免离线误报。

- [x] **Step 3: 实现运行时目标解析**

```rust
pub struct ResolvedTarget {
    pub provider: UiProvider,
    pub backend_id: String,
    pub model: String,
    pub api_key: String,
}

pub struct AuxiliaryTargets {
    pub preferred: ResolvedTarget,
    pub fallback: Option<ResolvedTarget>,
}
```

`auto` 时 preferred 就是 primary 且 fallback=None。显式 route 查 UI provider ID；provider 不存在、禁用或无凭据时 preferred=primary；有效时 preferred=explicit，若与 primary 不同则 fallback=Some(primary)。

- [x] **Step 4: 定义跨进程目标并随 ChatRequest 透传**

在 `common/src/auxiliary_target.rs`：

```rust
use crate::ChatTarget;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuxiliaryTask {
    TitleGeneration,
    Compaction,
    SmartApproval,
    Dreaming,
    BackgroundReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuxiliaryTargetChain {
    pub task: AuxiliaryTask,
    pub targets: Vec<ChatTarget>,
}
```

proto 增加 `AuxiliaryModelTarget`（task/provider_id/backend_id/model/api_key/base_url/order），并在 `ChatRequest` 增加 repeated 字段。`start_chat` 每次发送时解析五类任务：每类最多 preferred + fallback 两项。backend 构建或复用 AgentLoop 时更新其 `HashMap<AuxiliaryTask, Vec<ChatTarget>>`；不得落盘 API key。

AgentLoop 暴露：

```rust
pub fn auxiliary_targets(&self, task: AuxiliaryTask) -> Vec<ChatTarget>
```

没有传输目标时返回当前主 `ChatTarget`，保持旧客户端兼容。

- [x] **Step 5: 注册 commands 并编译**

Run: `cargo check -p astro-agent`  
Expected: PASS。

- [x] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri/src/auxiliary_commands.rs \
  apps/desktop/src-tauri/src/auxiliary_resolver.rs \
  apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/commands.rs \
  apps/desktop/src/types.ts common/src/auxiliary_target.rs common/src/lib.rs \
  proto/proto/astro.proto agent/src/runtime/mod.rs
git commit -m "feat(settings): expose auxiliary model routes"
```

---

### Task 3: 辅助模型设置面板

**Files:**
- Create: `apps/desktop/src/hooks/settings/useAuxiliarySettings.ts`
- Create: `apps/desktop/src/components/settings/AuxiliaryModelsPanel.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/styles/features/memory.css`

**Interfaces:**
- Consumes Task 2 commands/DTO.
- Produces five-row settings panel and provider-grouped model chooser.

- [x] **Step 1: 实现 hook**

hook 暴露：

```typescript
type UseAuxiliarySettings = {
  loading: boolean;
  error: string | null;
  settings: AuxiliarySettingsDto | null;
  setRoute(task: AuxiliaryTaskId, provider: string, model: string): Promise<void>;
  resetRoute(task: AuxiliaryTaskId): Promise<void>;
  resetAll(): Promise<void>;
  reload(): Promise<void>;
};
```

每个 mutation 使用 Tauri 返回的完整 settings 替换本地值；失败保留旧值并设置 error。

- [x] **Step 2: 实现五行面板**

固定顺序：

```typescript
const TASK_IDS: AuxiliaryTaskId[] = [
  "title_generation",
  "compaction",
  "smart_approval",
  "dreaming",
  "background_review",
];
```

每行显示 i18n 名称/说明、`displayLabel`、不可用警告、“设为主模型”和“更改”。顶部“全部重置为主模型”调用 `resetAll`。

- [x] **Step 3: 实现 Provider/Model 选择**

加载 `get_providers_state`，仅展示 enabled provider；展开 provider 时先用 `get_cached_provider_models`，为空再调用 `list_provider_models`。选择模型后一次提交 provider ID + model ID，不修改主模型。

- [x] **Step 4: 挂载设置导航并补 i18n**

新增“辅助模型”设置入口；不得把五项塞入 Provider Media tab。中英文文案覆盖五任务、自动、设为主模型、更改、全部重置、不可用降级说明。

- [x] **Step 5: 构建与冒烟**

Run: `cd frontend && npm run build`  
Expected: PASS。

手工检查：五行默认均显示主模型；指定模型后重启仍保留；全部重置不改变 `background_review_enabled`。

- [x] **Step 6: Commit**

```bash
git add apps/desktop/src/hooks/settings/useAuxiliarySettings.ts \
  apps/desktop/src/components/settings/AuxiliaryModelsPanel.tsx \
  apps/desktop/src/i18n/messages.ts apps/desktop/src/styles/features/memory.css \
  apps/desktop/src/App.tsx
git commit -m "feat(settings): add auxiliary models panel"
```

---

### Task 4: 上下文压缩使用辅助路由和主模型重试

**Files:**
- Modify: `apps/desktop/src-tauri/src/compaction_commands.rs`

**Interfaces:**
- Consumes: `resolve_auxiliary_targets(AuxiliaryKind::Compaction, primary)`.

- [x] **Step 1: 抽取可测试的两目标执行函数**

```rust
async fn summarize_with_targets(
    targets: AuxiliaryTargets,
    prompt: &str,
) -> Result<String, String> {
    match summarize_with_target(&targets.preferred, prompt).await {
        Ok(text) if !text.trim().is_empty() => Ok(text),
        Ok(_) | Err(_) => match targets.fallback {
            Some(fallback) => summarize_with_target(&fallback, prompt).await,
            None => Err("auxiliary compaction returned no summary".into()),
        },
    }
}
```

新增纯函数 `next_compaction_target(targets, failed_index)`，测试 preferred 失败后返回 fallback、fallback 失败后返回 `None`；实际 provider 调用按该顺序循环，避免依赖网络 mock。

- [x] **Step 2: 接入 compaction**

用 active UI provider 构造 primary，再解析 `AuxiliaryKind::Compaction`。只有两次模型调用均失败才进入现有启发式摘要，并保持 `CompactChatResultDto.degraded = true`。

- [x] **Step 3: 验证**

Run: `cargo test -p astro-agent compaction -- --nocapture`  
Expected: PASS。

Run: `cargo check -p astro-agent`  
Expected: PASS。

- [x] **Step 4: Commit**

```bash
git add apps/desktop/src-tauri/src/compaction_commands.rs
git commit -m "feat(compaction): route summaries through auxiliary model"
```

---

### Task 5: 智能审批使用辅助路由

**Files:**
- Modify: `crates/agent-core/src/control/smart_approval.rs`
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`

**Interfaces:**
- Keeps: `ASTRO_SMART_APPROVAL` as master enable switch.
- Produces: preferred/fallback completion targets accepted by approval evaluator.

- [x] **Step 1: 写失败回退测试**

在 `smart_approval.rs` tests 增加带 mock completion closure 的测试：preferred 返回 provider error，fallback 返回 `ASK`，断言 verdict 为 Ask；两者失败时返回原始 Ask，不自动 allow。

- [x] **Step 2: 重构 evaluator 接受目标序列**

将单个 session provider 参数改为 `&[ApprovalTarget]`，最多两个目标。循环尝试，首个可解析 verdict 返回；全部失败返回 error，由调用方保留原 Ask。

- [x] **Step 3: 在 tools_exec 解析 SmartApproval route**

保持 `smart_approval_enabled()` 判断；开启后直接读取 Task 2 已注入 AgentLoop 的 `AuxiliaryTask::SmartApproval` 目标序列。agent 不读取 keyring，也不依赖 `apps/desktop/src-tauri`。

- [x] **Step 4: 运行 agent 测试**

Run: `cargo test -p agent control::smart_approval -- --nocapture`  
Expected: PASS。

Run: `cargo test -p agent -- --nocapture`  
Expected: PASS。

- [x] **Step 5: Commit**

```bash
git add agent/src/control/smart_approval.rs agent/src/streaming/tools_exec.rs
git commit -m "feat(approval): use configured auxiliary model"
```

---

### Task 6: 统一入梦与记忆审查的凭据和降级

**Files:**
- Modify: `apps/desktop/src-tauri/src/dreaming_commands.rs`
- Modify: `crates/agent-core/src/exec/memory_review.rs`
- Modify: `crates/agent-core/src/runtime/mod.rs`

**Interfaces:**
- Dreaming uses `AuxiliaryKind::Dreaming`.
- Background review uses `AuxiliaryKind::BackgroundReview`.

- [x] **Step 1: 修复 dreaming 静默混用**

删除 `find_provider_by_backend(...).unwrap_or(ui)`。显式 provider ID 必须通过统一 resolver：不可用时明确选择 primary；有效时使用该 provider 自己的 endpoint/key/model。首选调用失败后重试 primary 一次。

- [x] **Step 2: 修复 background review 凭据**

当前 review job 不能用 session `api_key/base_url` 搭配显式 provider backend。将 job 从 AgentLoop 的 `AuxiliaryTask::BackgroundReview` 目标序列复制完整 `ChatTarget`；按 preferred、fallback 顺序调用。两次失败只记录 review failure event，不影响 Done。

- [x] **Step 3: 增加测试**

测试显式路由使用显式目标凭据、显式失败回退 session 目标、两次失败不 panic。

- [x] **Step 4: 验证**

Run: `cargo test -p agent exec::memory_review -- --nocapture`  
Expected: PASS。

Run: `cargo check -p astro-agent`  
Expected: PASS。

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/dreaming_commands.rs \
  agent/src/exec/memory_review.rs agent/src/runtime/mod.rs
git commit -m "fix(auxiliary): retry dreaming and review with main model"
```

---

### Task 7: 首轮异步标题生成与会话元数据事件

**Files:**
- Create: `crates/agent-core/src/exec/title_generation.rs`
- Modify: `crates/agent-core/src/exec/mod.rs`
- Modify: `crates/agent-server/src/grpc/astro_service.rs`
- Modify: `crates/agent-server/src/session_events.rs`
- Modify: `proto/proto/astro.proto`
- Modify: `apps/desktop/src-tauri/src/session_events.rs`
- Modify: `apps/desktop/src/components/chat/ChatSessionList.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`

**Interfaces:**
- Consumes session plan: `first_turn_text`, `set_session_title_if_empty`, `dispatchSessionsChanged`.
- Produces: `SessionMetadataChanged { session_id, title }`.
- Produces command: `regenerate_session_title`.

- [x] **Step 1: 写标题纯函数测试**

```rust
#[test]
fn sanitize_title_removes_wrappers_and_limits_chars() {
    assert_eq!(sanitize_title(" **「Rust 会话管理」**\n解释", 20), "Rust 会话管理");
}

#[test]
fn empty_title_is_rejected() {
    assert!(sanitize_title(" \n ** ", 20).is_empty());
}
```

另加 Store 并发测试：生成任务读取空标题后，手动标题先写入，`set_session_title_if_empty` 返回 false 且保留手动标题。

- [x] **Step 2: 实现 title generation executor**

`spawn_title_generation_after_turn`：

1. 在 `tokio::spawn` 内打开 `SessionStore`。
2. 若 title 已存在立即返回。
3. 调 `first_turn_text`，缺少完整首轮返回。
4. 用 `AuxiliaryKind::TitleGeneration` 解析 preferred/fallback。
5. prompt 明确只返回一个短标题。
6. `sanitize_title` 清理 Markdown、引号、换行并截断 40 Unicode chars。
7. `set_session_title_if_empty` 成功后向 SessionEvent hub 发送 metadata changed。

- [x] **Step 3: 扩展 proto 和事件桥**

在 `SessionEvent` oneof 增加 `session_metadata_changed`，载荷至少含 `session_id` 和 `title`。同步 backend hub、Tauri DTO 与前端类型。运行 proto 生成使用仓库现有 `build.rs`，不得手改生成文件。

- [x] **Step 4: 在首轮 Done 后触发**

`astro_service.rs` 的 `is_done` 分支与 background review 并列调用 spawn。任务本身通过 title 是否为空和 first turn 是否完整保证只执行一次。

- [x] **Step 5: 实现手动重新生成 command**

Tauri command 复用相同 prompt/清理，但最终调用 `set_session_title` 强制覆盖。无完整首轮返回明确错误。成功后 emit metadata event。

- [x] **Step 6: 前端刷新和启用菜单**

`ChatSessionList` 收到 metadata event 后更新匹配项 summary，或调用 `loadSessions()`；启用会话管理计划中暂时 disabled 的“重新生成标题”。生成失败用非阻塞错误。

- [x] **Step 7: 验证**

Run:

```bash
cargo test -p session -- --nocapture
cargo test -p agent title_generation -- --nocapture
cargo test -p backend session_events -- --nocapture
cargo check -p astro-agent
cd frontend && npm run build
```

Expected: 全部 PASS。

- [x] **Step 8: Commit**

```bash
git add agent/src/exec/title_generation.rs agent/src/exec/mod.rs \
  backend/src/grpc/astro_service.rs backend/src/session_events.rs \
  proto/proto/astro.proto apps/desktop/src-tauri/src/session_events.rs \
  apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs \
  apps/desktop/src/components/chat/ChatSessionList.tsx apps/desktop/src/i18n/messages.ts
git commit -m "feat(chat): generate session titles after first turn"
```

---

### Task 8: 文档、全量回归与冒烟

**Files:**
- Modify: `docs/memory.md`
- Modify: `docs/superpowers/specs/2026-07-16-session-management-auxiliary-models-design.md`

- [x] **Step 1: 文档化配置**

在 `docs/memory.md` 写出五类 YAML key、`auto/auto` 语义、显式 Provider ID、主模型重试和 background review 开关。明确网页抓取、技能搜索和 MCP 不使用辅助模型。

- [x] **Step 2: 运行全量相关验证**

```bash
cargo test -p memory -- --nocapture
cargo test -p session -- --nocapture
cargo test -p agent -- --nocapture
cargo test -p backend -- --nocapture
cargo check -p astro-agent
cd frontend && npm run build
```

Expected: 全部 PASS。

- [x] **Step 3: 手工集成冒烟**

验证：

1. 五项设为 auto 时使用当前主模型。
2. compaction 指定其它模型可成功压实。
3. 显式模型不可用时警告并回退主模型。
4. smart approval 总开关关闭时不调用任何辅助模型。
5. dreaming/review 显式模型使用自己的 endpoint/key。
6. 新会话首轮后自动标题；手动重命名不会被迟到任务覆盖。
7. 主动重新生成可以覆盖现有标题。

- [x] **Step 4: 更新 spec 状态并提交**

```bash
git add docs/memory.md \
  docs/superpowers/specs/2026-07-16-session-management-auxiliary-models-design.md
git commit -m "docs: document auxiliary model routing"
```
