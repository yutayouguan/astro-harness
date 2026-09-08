# Prompt 工程指南

> **工作原则继承**：全局使用 `AGENTS.md`，项目使用 根目录 `AGENTS.md`；两层原文同时加载，项目补充且冲突时优先，其他全局规则保留。此覆盖关系仅限工作原则层，不替换 SOUL/IDENTITY/USER/TOOLS，也不改写源文件或扩大授权。

> **协作与续接基线（2026-09-08）**：`agent-core/src/prompt/behavior_guidance.md` 仅保留授权、证据与运行时协议底线。身份、表达、用户偏好、工作方法和环境备忘分别由 `IDENTITY.md`、`SOUL.md`、`USER.md`、`AGENTS.md`、`TOOLS.md` 维护；默认正文位于 `agent-home/src/workspace/templates/*.md`。线程 notes 作为独立 user 上下文注入，history 回读 canonical rollout，显式压缩在完整工具结果批次持久化之后执行。实现与验收见 [线程检查点与上下文续接](../../04-详细设计阶段/03-记忆与上下文/07-线程检查点与上下文续接.md)。下文通用“先列方案确认”等历史示例不覆盖当前运行时契约。

> **Harness 边界（2026-09-07）**：Prompt scaffold 是 Harness 交给 Model 的可见工作面，但不等于 Harness。当前契约分为 `PromptContract.base_instructions`（对应 Responses 的 `instructions`）、用户输入/上下文消息/reasoning/assistant output/tool call/tool output 位于 `ResponsesRequest.input`、工具 schema 位于独立 `tools` 字段。三者不能通过拼接 system 文本互相替代。下方 Tera 模板引擎和 `agent-evals` 框架为历史设计方案，当前实现使用 `agent-core/src/prompt/` 模块和 `agent-evolution` crate。总体边界见 [Agent Harness 总体架构](11-Agent-Harness总体架构.md)。

> 阶段：系统设计 | 状态：定稿 | 说明：Tera 模板引擎、模型差异、反模式、测试方法

本文档面向**开发者**，指导如何为 astro-agent 编写 Skill Prompt 和系统 Prompt。这不是 Prompt 工程的理论综述，而是实际可用的模板、规范和注意事项。

---

## 系统 Prompt 设计原则

### 角色定义

简洁具体，不超过 3 句话。避免宏大叙述，直接说明模型在当前工作区中的定位。

```
# 不推荐
你是一个强大的 AI 助手，拥有无限可能，可以帮助用户完成任何任务。

# 推荐
你是用户的编程助手，专注于 Rust 和 TypeScript 项目开发。
你有权限读写项目文件、执行终端命令，并调用代码分析工具。
```

### 能力边界

明确告诉模型能做什么、不能做什么，减少越界行为。

```
## 你可以做的事
- 读写工作区目录内的文件
- 运行 cargo、npm、git 等开发工具
- 搜索知识库和互联网

## 你不可以做的事
- 访问工作区目录之外的文件
- 执行需要 sudo 的命令
- 在未经用户确认的情况下删除文件
```

### 输出格式

明确指定格式可大幅减少解析失败率，尤其是需要结构化输出的场景。

```
## 输出要求
- 代码修改请使用 diff 格式（+/- 前缀）
- 任务执行结果请以 JSON 格式返回，schema 见下方
- 如果不确定，先列出方案让用户确认，而非直接执行
```

---

## 标准系统 Prompt 模板

以下是 `prompts/zh-CN/system.md` 的完整内容：

```markdown
你是 {{workspace_name}} 工作区的 AI 助手，由 astro-agent 驱动。

## 身份与能力

- **角色**：{{agent_role}}
- **当前工作区**：{{workspace_path}}
- **可用工具**：{{available_tools}}
- **语言**：默认使用中文回复，代码保持英文

## 行为准则

1. **优先使用工具**：能用工具完成的任务不依赖记忆，实际读取文件而非猜测内容
2. **逐步确认**：破坏性操作（删除、覆盖、部署）执行前必须明确告知用户
3. **预算意识**：注意当前任务预算（已用 {{budget_used}}，上限 {{budget_limit}}）
4. **简洁输出**：回复聚焦结论，避免冗余解释，代码示例优先于文字描述

## 上下文

- 当前时间：{{current_time}}
- 最近任务：{{recent_tasks}}
- 活跃知识库：{{knowledge_bases}}

## 输出约束

- Markdown 代码块需标注语言类型
- 文件路径使用绝对路径
- 如需 JSON 输出，严格遵循指定 schema，不添加额外字段
```

---

## Skill Prompt 设计

Skill Prompt 是可复用的任务模板，存放在 `prompts/zh-CN/skills/` 目录下。

### 语法规范

| 元素 | 语法 | 说明 |
| ---- | ---- | ---- |
| 输入变量 | `{{variable_name}}` | 运行时由 Skill 引擎替换 |
| 可选变量 | `{{variable_name \| default_value}}` | 变量为空时使用默认值 |
| 条件块 | `{% if condition %}...{% endif %}` | 条件渲染 |
| 列表循环 | `{% for item in list %}...{% endfor %}` | 循环渲染 |

### 模板引擎选型

**选定方案：Tera**

| 候选 | 优势 | 劣势 | 结论 |
| ---- | ---- | ---- | ---- |
| Tera | Jinja2 语法（`{{ }}`/`{% %}`）、Rust 原生、编译期模板验证、丰富过滤器 | 比 Handlebars 稍重 | **选定** |
| Handlebars | 轻量、逻辑少 | 不支持 `{% if %}`/`{% for %}` 条件循环语法 | 不满足需求 |
| 自实现 | 完全可控 | 维护成本高、功能不全 | 不推荐 |

选择 Tera 的核心理由：
1. 语法与文档中已定义的 `{{variable_name}}`、`{% if %}`、`{% for %}` 模板语法完全一致
2. 纯 Rust 实现，无外部依赖，编译期即可验证模板语法
3. 支持模板继承、过滤器管道（如 `{{ content | truncate(length=100) }}`）
4. 社区活跃，Cargo 下载量高，长期维护有保障

依赖配置：
```toml
[dependencies]
tera = "1"
```

使用示例：
```rust
use tera::{Tera, Context};

let mut tera = Tera::default();
tera.add_raw_template("system", &template_content)?;

let mut ctx = Context::new();
ctx.insert("persona", &soul_md_content);
ctx.insert("memory_snapshot", &memory_text);
ctx.insert("tools", &tool_schemas);

let rendered = tera.render("system", &ctx)?;
```

### Few-shot 示例写法

Few-shot 示例要**典型**且**多样**，覆盖不同场景而非同一场景的变体：

```markdown
## 示例

### 示例 1：简单查询
用户输入：查一下 tokio 最新版本
期望输出：
```json
{
  "query": "tokio latest version",
  "sources": ["crates.io", "github"],
  "depth": "shallow"
}
```

### 示例 2：深度研究
用户输入：调研 Rust 异步运行时的主流选择，对比优缺点
期望输出：
```json
{
  "query": "Rust async runtime comparison tokio async-std smol",
  "sources": ["docs", "github", "blog"],
  "depth": "deep",
  "format": "comparison_table"
}
```
```

### Chain-of-Thought 触发

在需要推理的任务中，显式要求模型逐步思考：

```markdown
在给出最终答案之前，请一步步思考：
1. 用户的真实需求是什么？
2. 现有信息是否足够？缺少哪些关键信息？
3. 最合适的方案是什么？有哪些备选？
4. 方案的潜在风险是什么？

完成思考后，按以下格式输出结果：
```

### JSON 输出的 Schema 约束

```markdown
请严格按照以下 JSON Schema 输出，不允许添加 schema 中未定义的字段：

```json
{
  "$schema": "http://json-schema.org/draft-07/schema",
  "type": "object",
  "required": ["action", "confidence"],
  "properties": {
    "action": {
      "type": "string",
      "enum": ["create", "update", "delete", "skip"]
    },
    "confidence": {
      "type": "number",
      "minimum": 0,
      "maximum": 1
    },
    "reason": {
      "type": "string"
    }
  }
}
```
```

---

## 完整 Skill Prompt 示例

以下是 `prompts/zh-CN/skills/research.md` 的完整内容：

```markdown
---
name: research
description: 对指定主题进行深度调研，综合多个来源给出结构化报告
version: "1.2.0"
input_schema:
  topic: string          # 调研主题
  depth: shallow|deep    # 调研深度，默认 deep
  language: string       # 输出语言，默认 zh-CN
  max_sources: number    # 最多引用来源数，默认 10
---

# 调研任务

## 目标

对以下主题进行{{depth | deep}}调研：

**主题**：{{topic}}

## 执行步骤

请一步步思考并执行：

1. **拆解主题**：将 `{{topic}}` 分解为 3-5 个关键子问题
2. **信息收集**：使用 `web_search` 和 `knowledge_base_search` 工具检索每个子问题，最多引用 {{max_sources | 10}} 个来源
3. **信息过滤**：优先选择官方文档、学术论文、知名技术博客，排除过时内容（超过 2 年的内容需标注日期）
4. **综合分析**：对收集的信息进行交叉验证，标注信息的确定性（高/中/低）
5. **结构化输出**：按下方格式输出报告

## 输出格式

请用 {{language | zh-CN}} 输出以下结构的报告：

```json
{
  "topic": "{{topic}}",
  "summary": "150字以内的核心结论",
  "key_findings": [
    {
      "point": "关键发现",
      "confidence": "high|medium|low",
      "source": "来源 URL 或名称"
    }
  ],
  "details": {
    "background": "背景介绍（Markdown）",
    "analysis": "深度分析（Markdown）",
    "comparison": "横向对比（如适用，使用 Markdown 表格）",
    "limitations": "当前调研的局限性"
  },
  "recommendations": ["建议1", "建议2"],
  "sources": [
    {
      "title": "来源标题",
      "url": "URL",
      "accessed_at": "访问日期"
    }
  ]
}
```

## 示例

### 示例 1：技术选型调研

用户：调研 Rust GUI 框架的主流选择

思考过程：
- 子问题：有哪些主流框架？各自的成熟度？性能对比？跨平台支持？社区活跃度？
- 检索：搜索 "Rust GUI framework 2024 comparison"
- 过滤：优先 GitHub stars > 1k 的项目，官方文档完整的

输出（节选）：
```json
{
  "topic": "Rust GUI 框架选型",
  "summary": "tauri（Web 技术栈）、egui（即时模式）、iced（Elm 架构）是当前最成熟的三个选择。tauri 生态最完善，egui 性能最佳，iced 架构最优雅。",
  "key_findings": [
    {
      "point": "tauri 2.0 已稳定，支持移动端",
      "confidence": "high",
      "source": "https://tauri.app/blog/tauri-2-0-0-released/"
    }
  ]
}
```

### 示例 2：市场调研

用户：调研国内 AI 编程助手市场竞争格局

思考过程：
- 子问题：主要玩家？各自差异化？定价模式？用户反馈？
- 注意：区分 to-B 和 to-C 产品，关注最近 6 个月数据

{% if depth == "shallow" %}
## 浅度模式说明

当前为浅度调研（depth=shallow），将只执行前 3 个步骤，
输出 summary 和 key_findings，跳过 details 和 recommendations。
{% endif %}
```

---

## 不同模型的 Prompt 差异

| 模型 | 偏好结构 | 关键特点 | 注意事项 |
| ---- | ---- | ---- | ---- |
| Claude（Anthropic） | XML 标签（`<thinking>`、`<output>`） | system prompt 影响大，角色扮演响应好 | 善用 `<thinking>` 引导推理过程 |
| GPT（OpenAI） | Markdown 标题层级 | function calling 格式严格 | JSON schema 要精确，避免模糊描述 |
| Gemini（Google） | 自然语言为主 | 超长上下文（1M tokens），多模态强 | 中文理解略弱于英文，复杂指令建议英文 |
| DeepSeek | 中文效果极好 | 推理模型（deepseek-reasoner）内置 CoT | 推理模式下减少 few-shot，避免干扰推理链 |
| Qwen（阿里） | 中英文均衡 | 代码和中文都强，支持超长上下文 | 阿里巴巴业务场景数据丰富，电商/企业场景效果好 |

### Claude 专属技巧

```markdown
<!-- 利用 XML 标签结构化输出 -->
请按以下结构回复：

<thinking>
在这里思考用户的真实需求和最佳方案
</thinking>

<output>
在这里输出最终结果
</output>

<next_steps>
建议用户的下一步操作
</next_steps>
```

### GPT Function Calling 格式

```json
{
  "name": "search_knowledge_base",
  "description": "在工作区知识库中搜索相关内容",
  "parameters": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "搜索查询词，使用自然语言描述"
      },
      "top_k": {
        "type": "integer",
        "description": "返回结果数量，默认 5",
        "default": 5
      }
    },
    "required": ["query"]
  }
}
```

### DeepSeek 推理模式注意事项

```python
# deepseek-reasoner 会自动生成推理过程（reasoning_content）
# 不需要在 prompt 中要求"一步步思考"，避免重复
# 减少 few-shot 数量（0-1 个），过多示例会干扰推理链

# 推荐写法
prompt = """
分析以下代码的安全漏洞，给出具体的修复方案。

代码：
{code}
"""

# 不推荐写法（会与内置推理重复）
prompt = """
请一步步思考，分析以下代码的安全漏洞...
首先，第一步...
其次，第二步...
"""
```

---

## 常见 Prompt 反模式

### 1. 目标模糊

```markdown
# Bad：目标不清晰，模型无法判断成功标准
帮我优化这段代码

# Good：明确优化维度和验收标准
帮我优化这段代码的性能：
- 目标：将 process_batch() 的执行时间从当前 500ms 降低到 100ms 以内
- 约束：不改变函数签名，保持现有测试通过
- 优先考虑算法复杂度优化，其次考虑并发
```

### 2. 过度约束

```markdown
# Bad：过多限制导致模型无法发挥，输出模板化
必须用以下格式：第一步...第二步...第三步...
必须包含：背景、分析、结论、建议
每部分不少于200字，不超过300字
必须引用至少3个来源

# Good：给出目标，让模型自主决定最佳表达方式
请分析这个架构方案的利弊，重点关注扩展性和维护成本
```

### 3. Few-shot 示例质量差

```markdown
# Bad：示例都是同一类型，缺乏多样性
示例1：输入"写一首诗" → 输出一首五言诗
示例2：输入"写首诗" → 输出一首五言诗
示例3：输入"帮我写诗" → 输出一首五言诗

# Good：示例覆盖不同场景和边界情况
示例1：简单任务 → 直接执行
示例2：需要澄清的任务 → 先提问再执行
示例3：超出能力范围的任务 → 说明边界并给出替代方案
```

### 4. 忽略边界情况

```markdown
# Bad：只描述正常路径
将用户输入的 JSON 解析后存储到数据库

# Good：明确异常处理方式
将用户输入的 JSON 解析后存储到数据库。
- 如果 JSON 格式无效：返回具体的解析错误位置，不要静默失败
- 如果必填字段缺失：列出所有缺失字段，不要部分存储
- 如果数据库写入失败：回滚并报告错误，不要假装成功
```

### 5. 中英文混用不一致

```markdown
# Bad：系统 prompt 中英文混用，模型不知道该用哪种语言回复
你是一个 AI assistant，帮助用户 handle 各种 tasks。
请用 natural language 回复，保持 professional tone。

# Good：统一语言
你是一个 AI 助手，帮助用户处理各类任务。
请使用自然、专业的中文回复。代码和技术术语保留英文原文。
```

---

## Prompt 测试方法

> **注**：下方 `agent-evals` 命令为历史设计方案。当前评估功能由 `agent-evolution` crate（自进化/学习循环：改进提议、评判、信号分析、评估集、DSPy 集成）和配套 Python 包 `evolution-dspy/` 承载。

使用评估框架对 Prompt 进行系统评估：

```bash
# 运行指定 Skill 的评估套件
cargo run --bin agent-evals -- \
  --skill research \
  --cases evals/research_cases.jsonl \
  --judge claude-3-5-sonnet \
  --output evals/results/research_v1.2.json
```

评估用例格式（`evals/research_cases.jsonl`）：

```jsonl
{"id": "case_001", "input": {"topic": "Rust 内存安全机制"}, "expected": {"min_key_findings": 3, "has_sources": true, "confidence_distribution": "mixed"}}
{"id": "case_002", "input": {"topic": "不存在的技术XYZ"}, "expected": {"handles_unknown": true, "no_hallucination": true}}
{"id": "case_003", "input": {"topic": "GPT vs Claude 对比", "depth": "shallow"}, "expected": {"max_length": 500, "format": "shallow"}}
```

A/B 对比两个 Prompt 版本：

```bash
cargo run --bin agent-evals -- \
  --skill research \
  --cases evals/research_cases.jsonl \
  --prompt-a prompts/zh-CN/skills/research_v1.1.md \
  --prompt-b prompts/zh-CN/skills/research_v1.2.md \
  --judge claude-3-5-sonnet \
  --output evals/results/ab_test_research.json

# 输出示例：
# Prompt A 平均得分：7.2 / 10
# Prompt B 平均得分：8.6 / 10
# 建议采用 Prompt B（提升 +19.4%）
```

---

## 多语言 Prompt 策略

| 场景 | 推荐语言 | 原因 |
| ---- | ---- | ---- |
| 中文用户的日常任务 | 中文 prompt | 减少翻译开销，Claude/DeepSeek/Qwen 中文效果好 |
| 代码生成与分析 | 英文 prompt | 代码相关训练数据以英文为主，效果更稳定 |
| 使用 Gemini 的任务 | 英文 prompt | Gemini 英文理解显著优于中文 |
| 使用 GPT-4o 的任务 | 均可，建议英文 | 英文 prompt 在复杂推理任务上略优 |
| 使用 DeepSeek/Qwen 的任务 | 中文 prompt | 这两个模型针对中文做了专项优化 |

---

## Prompt 版本管理

> **注**：下方代码示例为历史设计方案。当前 Prompt 版本管理由 `agent-evolution` crate 承载。

astro-agent 使用 content-addressable 存储管理 Prompt 版本，每次修改自动生成新版本 hash，进化引擎优化时保留完整历史。

```rust
// crates/agent-evolution/src/prompt_store.rs（概念示例）

use sha2::{Sha256, Digest};

pub struct PromptVersion {
    pub hash: String,        // SHA-256 of content，作为版本 ID
    pub content: String,
    pub skill_name: String,
    pub created_at: DateTime<Utc>,
    pub eval_score: Option<f64>,  // 评估得分，进化引擎用于选优
    pub parent_hash: Option<String>, // 上一个版本的 hash
}

impl PromptVersion {
    pub fn new(skill_name: &str, content: &str) -> Self {
        let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        Self {
            hash,
            content: content.to_string(),
            skill_name: skill_name.to_string(),
            created_at: Utc::now(),
            eval_score: None,
            parent_hash: None,
        }
    }
}
```

数据库存储：

```sql
CREATE TABLE prompt_versions (
    hash        TEXT PRIMARY KEY,      -- SHA-256 of content
    skill_name  TEXT NOT NULL,
    content     TEXT NOT NULL,
    created_at  DATETIME NOT NULL,
    eval_score  REAL,                  -- 由 agent-evals 写入
    parent_hash TEXT,                  -- 版本链
    is_active   BOOLEAN DEFAULT FALSE  -- 当前激活版本
);

-- 查询某个 Skill 的版本历史（按时间倒序）
SELECT hash, created_at, eval_score, is_active
FROM prompt_versions
WHERE skill_name = 'research'
ORDER BY created_at DESC;
```

---

## 目录结构

```
prompts/
├── zh-CN/
│   ├── system.md                  # 标准系统 Prompt 模板
│   └── skills/
│       ├── research.md            # 调研 Skill
│       ├── code-review.md         # 代码评审 Skill
│       ├── summarize.md           # 内容摘要 Skill
│       └── translate.md           # 翻译 Skill
└── en-US/
    ├── system.md
    └── skills/
        └── research.md

evals/
├── research_cases.jsonl           # research Skill 评估用例
├── code-review_cases.jsonl
└── results/                       # 评估结果（git-ignored）
    ├── research_v1.2.json
    └── ab_test_research.json

crates/agent-evolution/              # 自进化/学习循环
crates/agent-core/src/prompt/       # 上下文组装、prompt builder、sanitization
```
