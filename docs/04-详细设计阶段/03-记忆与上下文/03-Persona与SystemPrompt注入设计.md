# Agent Persona 与 System Prompt 注入设计

> **Harness 当前基线（2026-09-07）**：当前 scaffold 契约是 `PromptContract { base_instructions, context, context_sections, usage }` + 用户输入 + 独立 `ResponsesRequest.tools`。SOUL/MEMORY/USER/daily/recall/Skills/AGENTS/TOOLS/interaction guidance 作为有来源的 dynamic context 装配。文件位置：SOUL.md / MEMORY.md / USER.md 均位于 `~/.astro/agents/{agent_id}/`，配置从 `~/.astro/config.toml` 和 `<project>/.astro/config.toml` 加载。本文其余固定 8 槽位和 `SystemPromptBuilder` 伪代码仅作历史设计。

> 版本：v1.0 | 日期：2026-08-07 | 状态：草稿
> 对应需求：F-11 Agent 人格配置（Persona）、M-13 Agent 人格配置模块

本文档描述 Astro Agent 的 System Prompt **确定性多槽位注入**机制：8 个槽位按固定顺序拼接，各组件（SOUL.md 身份、工作区 Prompt、记忆、Skills 指南等）独立可控、可叠加、可调试。

---

## 1. 三层人格模型

```text
┌─────────────────────────────────────────────────────────┐
│  Layer 1: 全局身份（SOUL.md）                           │
│           跨所有工作区的持久基线人格                    │
├─────────────────────────────────────────────────────────┤
│  Layer 2: 工作区 Prompt（workspace.system_prompt）      │
│           工作区级专业领域覆盖，叠加在全局身份之上     │
├─────────────────────────────────────────────────────────┤
│  Layer 3: 会话级覆盖（/personality）                    │
│           临时风格切换，不修改持久文件，会话结束清除   │
└─────────────────────────────────────────────────────────┘
```

三层递进叠加，下层不能覆盖或删除上层，只能在已有基线上追加说明。

---

## 2. System Prompt 8 槽位注入顺序

每次 LLM 调用前，`ContextBuilder` 按以下固定顺序拼接 system prompt：

| 槽位 | 内容来源 | 最大长度 | 是否必须 |
| ---- | -------- | -------- | -------- |
| **[1] SOUL.md** | `~/.astro/agents/{agent_id}/SOUL.md` 全文 | 4 000 tokens | 是（缺失时用内置默认） |
| **[2] 工作区 Prompt** | `workspace.system_prompt` 字段 | 2 000 tokens | 否 |
| **[3] 会话级人格** | `/personality` 切换的预设文本 | 500 tokens | 否（默认空） |
| **[4] 记忆注入** | L2 语义检索 + `MEMORY.md` + `USER.md` 快照 | 2 200 + 1 375 + 800 tokens | 是（空文件则跳过） |
| **[5] Skill 指南** | BM25 检索 top-5 Skill 摘要 | 1 500 tokens | 否（无相关 Skill 跳过） |
| **[6] 工具列表** | 当前可用工具的 JSON Schema 摘要 | 3 000 tokens | 是 |
| **[7] 自适应规划指令** | 固定的规划触发规则片段 | 300 tokens | 是 |
| **[8] 调试注入**（可选） | 调试模式下的额外说明 | 200 tokens | 否 |

> **工具 Schema Token 预算**：Slot [6] 上限 3,000 tokens。当可用工具的 schema 总量超过预算时，按以下策略截断：
>
> 1. 内置工具始终保留（约 1,500 tokens for 28 tools 的 name+description 摘要）
> 2. MCP/WASM 工具按 BM25 与当前查询的相关性排序，保留 Top-N 直到 token 预算用尽
> 3. 被截断的工具仍可通过 `tool_search` 工具按需发现和调用

### Slot [4] — 记忆注入（L2 + L3 + L4）

```xml
<memory>
  <!-- L3 持久记忆 -->
  <persistent>
    {MEMORY.md 快照，上限 2,200 tokens}
  </persistent>
  
  <!-- L4（用户建模） -->
  <user_profile>
    {USER.md 快照，上限 1,375 tokens}
  </user_profile>
  
  <!-- L2 语义检索（per-turn 动态注入） -->
  <semantic_recall>
    {KNN 向量检索结果，上限 800 tokens，按相关度排序}
  </semantic_recall>
</memory>
```

### 2.1 注入规则

- **槽位 1 冻结**：SOUL.md 在会话开始时读取快照，会话中途修改文件不影响当前会话（保护 Anthropic Prompt Cache 前缀稳定性）
- **槽位 4 冻结**：MEMORY.md / USER.md 同样在会话启动时一次性注入，会话中途写磁盘后下次会话才生效
- **槽位 2/3 热更新**：工作区 Prompt 和会话级人格可以中途通过 Tauri Command 更新，但更新后 Prompt Cache 会失效
- **超长截断**：各槽位超过最大长度时，从末尾截断并追加 `[...已截断]` 标记

---

## 3. SOUL.md 规范

### 3.1 文件位置与生命周期

```text
~/.astro/agents/{agent_id}/SOUL.md    # Agent 人格
~/.astro/agents/{agent_id}/MEMORY.md  # 项目记忆（快照）
~/.astro/agents/{agent_id}/USER.md    # 用户画像（快照）
~/.astro/config.toml                  # 全局统一配置（agent 设置 + MCP + custom_providers）
```

首次运行时自动生成默认 SOUL.md：

```markdown
# Identity

You are Astro, an AI assistant built to help developers think, build, and learn.

## Core Traits

- Direct and honest: give the answer, not a preamble
- Technically precise: use exact terms, code > prose for code questions
- Curious: ask clarifying questions when the task is ambiguous

## Communication Style

- Default: concise, code-first
- Adapt to the user's expertise level
- No unsolicited moral disclaimers
```

### 3.2 安全扫描与截断

SOUL.md 注入前经过安全扫描（见 [06-安全边界.md](../../03-系统设计阶段/03-基础设施/06-安全边界.md)），检测以下模式：
- Prompt 注入模式（`Ignore previous instructions`、`System:` 强制前缀等）
- 不可见 Unicode 字符（U+200B 等零宽字符）
- 超过 4 000 tokens 时从末尾截断

---

## 4. 会话级人格（/personality）

### 4.1 内置预设库

| 预设名 | 风格描述 | System Prompt 片段 |
| ------ | -------- | ------------------ |
| `helpful` | 默认助手风格（空，不追加） | — |
| `concise` | 极简，只给答案 | "Be maximally concise. One sentence when possible. Code over prose." |
| `technical` | 偏向技术深度，包含更多细节 | "Go deep on technical detail. Prefer precise terminology over plain language." |
| `creative` | 探索性思维，多角度发散 | "Think laterally. Offer multiple approaches. Explore creative alternatives." |
| `teacher` | 教学模式，分步骤解释 | "Teach step by step. Use analogies. Check understanding with follow-up questions." |
| `philosopher` | 苏格拉底式追问 | "Question assumptions. Explore first principles. Raise counterarguments." |
| `hype` | 充满热情的鼓励者 | "Be enthusiastic and encouraging. Celebrate small wins. Use energy." |
| `noir` | 黑色电影硬汉风格 | "Respond like a hard-boiled detective narrating a case. Dry wit required." |
| `pirate` | 海盗口音娱乐模式 | "Respond like a pirate. Arr. Keep it helpful though, matey." |

### 4.2 Tauri Command 接口

```typescript
// 前端 → 后端
invoke("set_personality", {
  conversationId: string,
  preset: string | null,   // null = 恢复 helpful（默认）
  custom?: string,         // 自定义人格文本（config.yaml 定义的命名人格）
})

// 后端 → 前端（确认切换）
interface PersonalityChangedPayload {
  preset: string;
  displayName: string;
}
// event: "personality_changed"
```

### 4.3 config.yaml 自定义人格

```yaml
# ~/.astro/config.toml (或 agent 级 config)

personalities:
  rust_reviewer:
    display_name: "Rust 代码审查员"
    prompt: |
      你是一名严格的 Rust 代码审查员。
      关注：内存安全、借用检查、panic! 风险、性能瓶颈。
      对每个问题指出：问题所在、为何是问题、如何修复。

  zh_translator:
    display_name: "中英翻译官"
    prompt: |
      你是专业翻译，专注中英技术文档互译。
      保留原文术语和格式，不意译技术名词。
```

自定义人格通过 `/personality rust_reviewer` 激活，与内置预设统一管理。

---

## 5. Rust 实现

### 5.1 SystemPromptBuilder

```rust
// crates/agent-core/src/prompt/system_prompt_builder.rs

pub struct SystemPromptBuilder {
    workspace_id: String,
    pool: SqlitePool,
    soul_path: PathBuf,
    memory_path: PathBuf,
}

impl SystemPromptBuilder {
    /// 按 8 槽位顺序构建完整 system prompt
    pub async fn build(&self, ctx: &SessionContext) -> anyhow::Result<String> {
        let mut parts: Vec<String> = Vec::with_capacity(8);

        // [1] SOUL.md（启动时已冻结快照，直接取缓存）
        parts.push(self.soul_snapshot(&ctx.soul_snapshot));

        // [2] 工作区 Prompt
        if let Some(ws_prompt) = &ctx.workspace_prompt {
            if !ws_prompt.trim().is_empty() {
                parts.push(truncate(ws_prompt, 2000, "工作区 Prompt"));
            }
        }

        // [3] 会话级人格
        if let Some(personality) = &ctx.active_personality {
            parts.push(truncate(&personality.prompt, 500, "人格"));
        }

        // [4] 记忆注入（L2 + L3 + L4）
        {
            let mut memory_parts: Vec<String> = Vec::new();
            // L3 持久记忆（启动时已冻结快照）
            if !ctx.memory_snapshot.is_empty() {
                memory_parts.push(format!("  <persistent>\n{}\n  </persistent>", ctx.memory_snapshot));
            }
            // L4（用户建模）（启动时已冻结快照）
            if !ctx.user_snapshot.is_empty() {
                memory_parts.push(format!("  <user_profile>\n{}\n  </user_profile>", ctx.user_snapshot));
            }
            // L2 语义检索（per-turn 动态注入，由 ContextBuilder 在 round_loop 中填充）
            if let Some(recall) = &ctx.semantic_recall {
                if !recall.is_empty() {
                    memory_parts.push(format!("  <semantic_recall>\n{}\n  </semantic_recall>", recall));
                }
            }
            if !memory_parts.is_empty() {
                parts.push(format!("<memory>\n{}\n</memory>", memory_parts.join("\n")));
            }
        }

        // [5] Skill 指南（按需注入，由 ContextBuilder 在每轮推理前填充）
        // 注意：此处留空占位，由 ContextBuilder::inject_skills() 在 round_loop 中动态填充

        // [6] 工具列表（由 ToolRegistry 生成）
        parts.push(ctx.tool_descriptions.clone());

        // [7] 自适应规划指令（固定片段）
        parts.push(ADAPTIVE_PLANNING_INSTRUCTION.to_string());

        // [8] 调试注入
        if ctx.debug_mode {
            parts.push("<debug>调试模式：每次工具调用前输出工具名和参数。</debug>".to_string());
        }

        Ok(parts.join("\n\n"))
    }

    fn soul_snapshot(&self, snapshot: &str) -> String {
        let scanned = security_scan(snapshot);  // 注入检测 + Prompt 注入过滤
        truncate(&scanned, 4000, "SOUL.md")
    }
}

const ADAPTIVE_PLANNING_INSTRUCTION: &str = r#"<planning_rules>
当任务预计需要 3 步以上工具调用、涉及不可逆操作、需要生成子 Agent 或影响多个工作区时，
先调用 submit_plan 工具提交执行计划，等待用户确认后再执行。不要使用 XML 标签输出计划。
</planning_rules>"#;
```

### 5.2 SessionContext（会话启动时初始化）

```rust
// crates/agent-core/src/runtime/mod.rs (目标设计；当前上下文由 PromptContract 管理)

pub struct SessionContext {
    pub soul_snapshot: String,          // 启动时读取 SOUL.md 快照（冻结）
    pub memory_snapshot: String,        // 启动时读取 MEMORY.md 快照（冻结，L3）
    pub user_snapshot: String,          // 启动时读取 USER.md 快照（冻结，L4）
    pub semantic_recall: Option<String>, // L2 语义检索结果（per-turn 动态注入，上限 800 tokens）
    pub workspace_prompt: Option<String>,
    pub active_personality: Option<PersonalityPreset>,
    pub tool_descriptions: String,
    pub debug_mode: bool,
}

impl SessionContext {
    pub async fn init(workspace_id: &str, config: &WorkspaceConfig) -> anyhow::Result<Self> {
        let soul_path = astro_dir().join("SOUL.md");
        let soul_snapshot = read_or_default(&soul_path, DEFAULT_SOUL).await;

        let memory_path = workspace_dir(workspace_id).join("MEMORY.md");
        let user_path   = workspace_dir(workspace_id).join("USER.md");

        Ok(Self {
            soul_snapshot,
            memory_snapshot: read_or_empty(&memory_path).await,
            user_snapshot:   read_or_empty(&user_path).await,
            semantic_recall: None,  // per-turn 由 ContextBuilder::inject_semantic_recall() 填充
            workspace_prompt: config.system_prompt.clone(),
            active_personality: None,
            tool_descriptions: String::new(),  // 由 ToolRegistry 填充
            debug_mode: config.debug_mode,
        })
    }
}
```

---

## 6. 调试模式：System Prompt 全文查看

调试模式下，对话界面侧边展示可折叠的调试面板，其中包含当前生效的完整 system prompt（按槽位分节展示）：

```text
┌─────────────────────────────────────────────────┐
│ System Prompt                         [复制全文] │
├─────────────────────────────────────────────────┤
│ [1] SOUL.md               847 tokens  [展开]    │
│ [2] 工作区 Prompt         312 tokens  [展开]    │
│ [3] 会话级人格            42 tokens   concise   │
│ [4] 记忆注入（L2+L3+L4）  1,204 tokens [展开]   │
│ [5] Skill 指南            680 tokens  3 个 Skill│
│ [6] 工具列表              1,893 tokens [展开]   │
│ [7] 规划指令              68 tokens   (固定)    │
│ ──────────────────────────────────────          │
│ 总计：5,046 / 200,000 tokens                    │
└─────────────────────────────────────────────────┘
```

通过 Tauri Command `get_system_prompt_debug` 获取各槽位内容。

---

## 7. 工作区配置存储

Persona 相关设置存储在工作区 YAML 配置文件：

```yaml
# ~/.astro/config.toml (或 agent 级 config)

name: "Rust 后端开发"
system_prompt: |
  你在此工作区专注于 Rust 后端开发。
  优先使用 tokio + axum 技术栈。
  遇到性能问题时，首先分析 CPU vs I/O bound 类型。

personalities:
  rust_reviewer:
    display_name: "Rust 代码审查员"
    prompt: "你是严格的 Rust 代码审查员..."

agent:
  name: "Astro (Rust)"
  avatar: "🦀"
```

工作区 Prompt 通过设置界面编辑，实时 `save_workspace_config` 写磁盘；人格切换通过 `/personality` 斜杠命令或设置界面下拉触发。

---

## 8. Persona 设置 UI

```text
┌─────────────────────────────────────────────────────────────┐
│ 人格（Persona）设置                                         │
├─────────────────────────────────────────────────────────────┤
│ SOUL.md（全局身份）                               [编辑文件] │
│ ┌─────────────────────────────────────────────────────┐    │
│ │ You are Astro，a AI assistant...                    │    │
│ └─────────────────────────────────────────────────────┘    │
│                                                             │
│ 会话级人格 (/personality)                                   │
│ 当前：helpful                          [切换预设 ▼]        │
│ 预设：helpful / concise / technical / creative / teacher   │
│       philosopher / hype / noir / pirate                   │
│                                                             │
│ 工作区 Prompt                                               │
│ ┌─────────────────────────────────────────────────────┐    │
│ │ 你在此工作区专注于 Rust 后端开发...                 │    │
│ └─────────────────────────────────────────────────────┘    │
│                                                             │
│ System Prompt 注入槽位预览                                  │
│ [1] SOUL.md  [2] 工作区  [3] 人格  [4] 记忆  [5→8] ...   │
│                                           [查看完整 System Prompt] │
└─────────────────────────────────────────────────────────────┘
```

---

## 9. 设计约束

- **槽位 1/4 部分冻结**：SOUL.md、MEMORY.md 和 USER.md 快照会话内不更新（保证 Prompt Cache 前缀稳定）；L2 语义检索部分为 per-turn 动态注入，不影响 cache 前缀
- **工作区 Persona 隔离**：切换工作区时 `active_personality` 重置为 `None`，工作区 Prompt 独立加载
- **人格不继承**：子 Agent 继承父级权限集，但不继承父级 `active_personality`（子 Agent 始终从 `helpful` 开始）
- **SOUL.md 安全扫描**：注入前检测 Prompt 注入模式，超长自动截断，不暴露原始扫描细节给前端

---

## 相关文档

- [04-Prompt工程.md](../../03-系统设计阶段/01-架构设计/04-Prompt工程.md) — Prompt 模板规范、模型差异处理
- [07-上下文管理.md](../../03-系统设计阶段/02-核心功能模块/07-上下文管理.md) — ContextSlot 枚举、ContextBuilder 消息优先级
- [01-持久化层.md](../../03-系统设计阶段/03-基础设施/01-持久化层.md) — SOUL.md / MEMORY.md / USER.md 文件路径约定
- [06-安全边界.md](../../03-系统设计阶段/03-基础设施/06-安全边界.md) — Prompt 注入防御、安全扫描实现
- [05-多工作区隔离.md](../../03-系统设计阶段/06-桌面端/05-多工作区隔离.md) — 工作区级配置文件隔离方案
- [02-agent-core详细设计.md](../01-核心引擎层/02-agent-core详细设计.md) — Skill 指南动态注入时机
