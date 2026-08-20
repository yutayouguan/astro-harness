# Skills 系统详细设计

> 版本：v2.0 | 日期：2026-08-10 | 状态：草稿
> 对应需求：F-04 Skills 系统、F-12 自我进化引擎、F-23 插件生态、F-30 子 Agent 派生
> 参考模型：Claude Code Skills Architecture
> 上游文档：[04-Skills系统.md](../../03-系统设计阶段/02-核心功能模块/04-Skills系统.md)、[06-自我进化引擎.md](../../03-系统设计阶段/02-核心功能模块/06-自我进化引擎.md)

---

## 1. 系统概述

### 1.1 Skills 定义

Skill 是 Astro Agent 的可插拔能力单元。每个 Skill 以一个**目录**为载体，目录内的 `SKILL.md` 文件作为入口点，通过 YAML frontmatter 声明元数据，Markdown 正文承载 Agent 执行时注入的指令上下文。

Skill 不是传统的可执行插件，而是**结构化的提示词模块**——它告诉 Agent "在这类场景下应该怎么做"，同时携带模板、脚本和示例资源。Skill 可由用户手写、工作流编辑器生成、进化引擎合成，也可从市场安装。

### 1.2 与 Claude Code Skills 的关系

本设计以 Claude Code 的 Skills 架构为主要参照，在目录结构、frontmatter 字段、动态上下文注入、字符串替换、调用控制、子 Agent 执行、Skill 作用域 Hooks 等方面与之对齐。在此基础上，我们保留并扩展了以下 Astro Agent 原创特性：

| 特性 | 来源 | 说明 |
|------|------|------|
| BM25/tantivy 语义召回 | Astro 原创 | `trigger_patterns` + BM25 索引，自动将相关 Skill 注入系统提示 |
| EvolutionEngine 集成 | Astro 原创 | 从任务轨迹中自动创建/精化/退役 Skill |
| 版本链管理 | Astro 原创 | semver + `skill_versions` 表 + 向后兼容检查 |
| SkillStatus 生命周期 | Astro 原创 | Draft/Ready/Published/Deprecated/Evolving 五态流转 |
| SkillTestRunner | Astro 原创 | 录制/回放测试框架 |

### 1.3 设计原则

1. **目录即能力**：Skill 是一个目录，不只是一个文件。支撑资源（模板、脚本、示例、测试录制）与 SKILL.md 同级存放。
2. **描述常驻、正文按需**：所有 Skill 的 `description` 和 `when_to_use` 常驻系统提示（供模型匹配），但正文内容仅在调用时加载。
3. **双通道触发**：Skill 可由用户通过 `/name` 手动调用，也可由模型根据描述自动匹配调用。两个通道可独立启停。
4. **文件系统为真相源**：Skill 的规范定义在文件系统上，SQLite 是索引缓存。文件变更即时生效。
5. **安全沙箱**：动态上下文注入的命令执行受安全策略约束，Skill 作用域的工具权限不可超出 Agent 已有权限。

### 1.4 模块结构

```text
crates/agent-core/src/skills/
├── mod.rs              # 公开 API
├── manifest.rs         # SkillManifest — SKILL.md 元数据解析
├── registry.rs         # SkillRegistry — BM25 索引 + 内存注册表 + 热重载
├── repo.rs             # SkillRepo — SQLite 持久化
├── loader.rs           # SkillLoader — 目录扫描、发现、加载
├── context_inject.rs   # 动态上下文注入 (!`command` 语法)
├── substitution.rs     # 字符串替换系统 ($ARGUMENTS 等)
├── invocation.rs       # 调用控制逻辑 (模型/用户/混合)
├── lifecycle.rs        # 内容生命周期管理 (加载/持久/压缩/去重)
├── overrides.rs        # Skill Overrides (settings.json 控制)
├── permissions.rs      # 工具权限控制 (allowed/disallowed-tools)
├── watcher.rs          # 文件监视器 (live change detection)
└── evolution.rs        # EvolutionEngine 集成入口

crates/agent-runtime/src/skills/
├── mod.rs
├── test_runner.rs      # SkillTestRunner — 录制/回放测试
├── subagent.rs         # Subagent 执行 (context: fork)
└── hooks_bridge.rs     # Skill 作用域 Hooks 桥接
```

---

## 2. Skill 目录结构

### 2.1 单 Skill 目录布局

每个 Skill 是一个以 Skill 名称命名的目录，内含必选的 `SKILL.md` 入口文件和可选的辅助资源：

```text
my-skill/
├── SKILL.md              # 入口文件（必选）
├── template.md           # 模板文件，供 Agent 填充使用（可选）
├── templates/            # 多模板目录（可选）
│   ├── report.md
│   └── summary.md
├── examples/             # 示例输出（可选）
│   ├── good-output.md
│   └── edge-case.md
├── scripts/              # 可执行脚本（可选，供 !`command` 引用）
│   ├── lint-check.sh
│   └── validate.py
├── tests/                # 测试用例录制（可选）
│   ├── basic.recording.json
│   └── edge-case.recording.json
└── assets/               # 静态资源（可选）
    └── prompt-template.txt
```

`SKILL.md` 是唯一的必选文件。系统通过扫描目录下是否存在 `SKILL.md` 来识别 Skill 目录。

### 2.2 全局文件系统布局

```text
~/.astro/
├── skills/                          # 全局 Skill（所有工作区共享）
│   ├── global-linter/
│   │   └── SKILL.md
│   └── translate/
│       └── SKILL.md
├── settings.json                    # 全局设置（含 skillOverrides）
└── workspaces/<workspace_id>/
    └── .astro/
        ├── skills/                  # 工作区级 Skill
        │   ├── deploy-vercel/
        │   │   ├── SKILL.md
        │   │   └── scripts/
        │   │       └── check-env.sh
        │   └── code-review/
        │       ├── SKILL.md
        │       └── examples/
        │           └── sample-review.md
        └── settings.json           # 工作区设置（含 skillOverrides）

<app-binary>/
└── resources/
    └── builtin-skills/              # 内置 Skill（只读，随二进制分发）
        ├── summarize/
        │   └── SKILL.md
        └── init/
            └── SKILL.md
```

### 2.3 Monorepo 支持

对于 monorepo 项目，子目录可以拥有自己的 `.astro/skills/` 目录。当 Agent 的工作上下文位于某子目录时，该子目录的 Skill 优先于工作区根目录的同名 Skill。

```text
my-monorepo/
├── .astro/skills/           # 工作区根级
│   └── deploy/SKILL.md
├── apps/
│   └── web/
│       └── .astro/skills/   # 子目录级（仅在 apps/web 下激活）
│           └── deploy/SKILL.md   # 覆盖根级的 deploy
└── packages/
    └── shared/
        └── .astro/skills/
            └── lint/SKILL.md
```

激活规则：当 Agent 的 `cwd` 或当前操作文件路径匹配 `paths` 限定时，子目录 Skill 自动激活并覆盖同名的上级 Skill。

---

## 3. SKILL.md 格式规范

### 3.1 完整字段参考

```yaml
---
# ══════════════════════════════════════════
# 基础信息（Claude Code 对齐字段）
# ══════════════════════════════════════════
name: deploy-vercel                   # 必选。小写连字符，目录内唯一
description: >-                       # 必选。一句话说明，常驻系统提示（限 120 token）
  一键将 Next.js 项目部署到 Vercel，包含构建检查和回滚步骤
when_to_use: >-                       # 可选。详细的使用场景描述（辅助模型匹配）
  用户要求部署前端项目、发布到 Vercel、上线 Next.js 应用时使用

# ══════════════════════════════════════════
# 调用控制（Claude Code 对齐字段）
# ══════════════════════════════════════════
disable-model-invocation: false       # true = 仅用户可通过 /name 调用，模型不会自动触发
                                      # 适用于有副作用的 Skill（如 /deploy, /publish）
user-invocable: true                  # false = 仅模型可调用，用户在 / 菜单中看不到
                                      # 适用于背景知识型 Skill
allowed-tools: >-                     # 空格分隔。Skill 激活时额外授予的工具权限
  shell_exec file_write http_request
disallowed-tools: >-                  # 空格分隔。Skill 激活时屏蔽的工具
  git_push memory_write

# ══════════════════════════════════════════
# 执行控制（Claude Code 对齐字段）
# ══════════════════════════════════════════
model: claude-sonnet-4-20250514       # 可选。覆盖当前会话的模型
effort: high                          # 可选。推理努力级别 (low / medium / high)
context: fork                         # 可选。"fork" = 在子 Agent 中执行
agent: explore                        # 可选。context=fork 时指定子 Agent 类型
                                      # (explore / plan / general-purpose / custom)
shell: bash                           # 可选。脚本执行使用的 shell (bash / powershell / zsh)

# ══════════════════════════════════════════
# 参数系统（Claude Code 对齐字段）
# ══════════════════════════════════════════
arguments:                            # 命名位置参数定义
  - name: project_dir
    description: 项目根目录
    required: false
    default: "."
  - name: environment
    description: 部署环境
    required: false
    default: "preview"
argument-hint: "<project_dir> [environment]"  # 自动补全提示

# ══════════════════════════════════════════
# 作用域限定（Claude Code 对齐字段）
# ══════════════════════════════════════════
paths:                                # glob 模式，限定 Skill 激活的文件/目录范围
  - "apps/web/**"
  - "packages/ui/**"

# ══════════════════════════════════════════
# Skill 作用域 Hooks（Claude Code 对齐字段）
# ══════════════════════════════════════════
hooks:
  before_tool_execute:
    command: "scripts/lint-check.sh $TOOL_ARGS"
    on_failure: abort                 # abort | continue | skip
    timeout_ms: 5000
  after_tool_execute:
    command: "scripts/post-validate.sh"
    on_failure: continue

# ══════════════════════════════════════════
# Astro 扩展字段
# ══════════════════════════════════════════
version: "1.2.0"                      # semver 版本号
origin: imported                      # builtin | synthesized | imported | marketplace
status: ready                         # draft | ready | published | deprecated | evolving

trigger_patterns:                     # BM25 召回关键词（Astro 独有）
  - "部署到 vercel"
  - "deploy to vercel"
  - "发布前端"
trigger_threshold: 0.6                # BM25 得分阈值（默认 0.5）

risk_level: L2                        # 风险等级 L0-L3（影响人工审批策略）
requires_tools:                       # 声明依赖的工具（用于权限预检）
  - shell_exec
  - file_read
exec_backend: local                   # 执行后端 (local | docker)
timeout_secs: 300                     # 执行超时

tags: [deploy, vercel, nextjs]        # 分类标签（辅助 BM25 检索）
parent_skill: deploy-v1               # 进化链前驱 Skill（可选）
deprecates: deploy-legacy             # 本 Skill 替代的旧 Skill（可选）

# ══════════════════════════════════════════
# 内置测试用例
# ══════════════════════════════════════════
test_cases:
  - name: "基本部署"
    params: { project_dir: ".", environment: "preview" }
    expect_exit_code: 0
    expect_output_contains: "https://"
  - name: "生产部署"
    params: { project_dir: ".", environment: "production" }
    expect_exit_code: 0

# ══════════════════════════════════════════
# 参数 Schema（可选，用于 GUI 渲染和校验）
# ══════════════════════════════════════════
parameters:
  project_dir:
    type: string
    description: 项目根目录
    default: "."
  production:
    type: boolean
    description: 是否部署到生产环境
    default: false
---

# 部署到 Vercel

当前项目结构：
!`find . -name "*.json" -maxdepth 2 | head -20`

## 前置检查

1. 确认 `vercel.json` 或 `next.config.js` 存在
2. 检查环境变量 `VERCEL_TOKEN` 是否已设置
3. 运行 `npm run build` 验证构建

## 部署步骤

根据 `$environment` 参数选择部署方式：
- preview: `vercel`
- production: `vercel --prod`

## 回滚策略

若部署失败，执行 `vercel rollback` 恢复上一个成功部署。

## 输出要求

部署完成后输出：
- 部署 URL
- 构建耗时
- 部署状态
```

### 3.2 字段分类说明

| 分类 | 字段 | 必选 | 来源 |
|------|------|------|------|
| 基础 | `name`, `description` | 是 | Claude Code |
| 基础 | `when_to_use` | 否 | Claude Code |
| 调用 | `disable-model-invocation`, `user-invocable` | 否 | Claude Code |
| 权限 | `allowed-tools`, `disallowed-tools` | 否 | Claude Code |
| 执行 | `model`, `effort`, `context`, `agent`, `shell` | 否 | Claude Code |
| 参数 | `arguments`, `argument-hint` | 否 | Claude Code |
| 作用域 | `paths` | 否 | Claude Code |
| Hooks | `hooks` | 否 | Claude Code |
| 版本 | `version`, `origin`, `status` | 否 | Astro |
| 召回 | `trigger_patterns`, `trigger_threshold` | 否 | Astro |
| 安全 | `risk_level`, `requires_tools`, `exec_backend` | 否 | Astro |
| 测试 | `test_cases` | 否 | Astro |
| 进化 | `parent_skill`, `deprecates`, `tags` | 否 | Astro |

### 3.3 最小 Skill 示例

```markdown
---
name: summarize
description: 对给定文本生成结构化摘要
---

请对以下内容生成摘要，包含：
1. 核心观点（3-5 条）
2. 关键数据
3. 行动建议
```

仅 `name` 和 `description` 为必选字段，其余均有合理默认值。

---

## 4. 动态上下文注入

### 4.1 设计思想

动态上下文注入允许 SKILL.md 正文中嵌入 shell 命令，在 Skill 加载时（而非由 Agent 执行时）自动运行并将输出替换到正文中。这使 Skill 能携带当前环境的实时信息（如目录结构、git 状态、配置文件内容）。

### 4.2 内联语法

在 Markdown 正文中使用反引号包裹、以 `!` 前缀标记的命令：

```markdown
当前 Git 分支：!`git branch --show-current`
最近提交：!`git log --oneline -5`
```

加载时，系统执行 `git branch --show-current` 和 `git log --oneline -5`，将输出替换到正文对应位置。Agent 看到的是替换后的结果。

### 4.3 多行块语法

使用 ` ```! ` 围栏代码块执行多行命令：

````markdown
项目依赖列表：

```!
cat package.json | jq '.dependencies | keys[]' 2>/dev/null || echo "非 Node.js 项目"
ls Cargo.toml 2>/dev/null && cargo metadata --format-version 1 | jq '.packages[0].name'
```
````

整个代码块作为一个脚本执行，stdout 输出替换代码块本身。

### 4.4 Rust 实现

```rust
// crates/agent-core/src/skills/context_inject.rs

use regex::Regex;
use std::path::Path;
use tokio::process::Command;

/// 动态上下文注入配置
pub struct ContextInjectionConfig {
    /// 命令执行超时（毫秒）
    pub timeout_ms: u64,
    /// 单条命令最大输出长度（字节）
    pub max_output_bytes: usize,
    /// 允许执行的命令白名单模式（正则）
    pub allowed_commands: Vec<Regex>,
    /// 禁止执行的命令黑名单模式
    pub blocked_commands: Vec<Regex>,
    /// 执行 shell (bash / zsh / powershell)
    pub shell: String,
    /// 工作目录
    pub cwd: PathBuf,
}

impl Default for ContextInjectionConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 5_000,
            max_output_bytes: 32_768,  // 32KB
            allowed_commands: vec![],   // 空 = 允许所有
            blocked_commands: vec![
                Regex::new(r"(?i)\brm\s+-rf\b").unwrap(),
                Regex::new(r"(?i)\bcurl\b.*\|\s*sh").unwrap(),
                Regex::new(r"(?i)\bsudo\b").unwrap(),
            ],
            shell: "bash".into(),
            cwd: std::env::current_dir().unwrap_or_default(),
        }
    }
}

/// 处理 SKILL.md 正文中的动态注入占位符
pub async fn process_dynamic_injection(
    content: &str,
    config: &ContextInjectionConfig,
) -> anyhow::Result<String> {
    let mut result = content.to_string();

    // Phase 1: 处理多行块 ```! ... ```
    let block_re = Regex::new(r"```!\n([\s\S]*?)\n```")?;
    for cap in block_re.captures_iter(content) {
        let full_match = cap.get(0).unwrap().as_str();
        let script = &cap[1];

        let output = execute_injection_command(script, config).await?;
        result = result.replace(full_match, &output);
    }

    // Phase 2: 处理内联 !`command`
    let inline_re = Regex::new(r"!`([^`]+)`")?;
    let content_snapshot = result.clone();
    for cap in inline_re.captures_iter(&content_snapshot) {
        let full_match = cap.get(0).unwrap().as_str();
        let cmd = &cap[1];

        let output = execute_injection_command(cmd, config).await?;
        result = result.replacen(full_match, &output.trim(), 1);
    }

    Ok(result)
}

/// 执行注入命令（带安全检查和超时）
async fn execute_injection_command(
    command: &str,
    config: &ContextInjectionConfig,
) -> anyhow::Result<String> {
    // 安全检查：黑名单
    for blocked in &config.blocked_commands {
        if blocked.is_match(command) {
            return Ok(format!("[BLOCKED: 命令被安全策略拦截]"));
        }
    }

    // 安全检查：白名单（若配置了白名单，命令必须匹配）
    if !config.allowed_commands.is_empty() {
        let allowed = config.allowed_commands.iter().any(|r| r.is_match(command));
        if !allowed {
            return Ok(format!("[BLOCKED: 命令不在白名单中]"));
        }
    }

    let output = tokio::time::timeout(
        Duration::from_millis(config.timeout_ms),
        Command::new(&config.shell)
            .arg("-c")
            .arg(command)
            .current_dir(&config.cwd)
            .output(),
    ).await??;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let truncated = if stdout.len() > config.max_output_bytes {
        format!(
            "{}...\n[输出截断：超过 {} 字节]",
            &stdout[..config.max_output_bytes],
            config.max_output_bytes
        )
    } else {
        stdout.to_string()
    };

    Ok(truncated)
}
```

### 4.5 安全约束

| 约束 | 说明 |
|------|------|
| 命令黑名单 | `rm -rf`、`sudo`、`curl\|sh` 等危险模式默认拦截 |
| 执行超时 | 单条命令最长 5 秒，超时返回 `[TIMEOUT]` |
| 输出限制 | 单条命令最大 32KB 输出，超出截断 |
| 执行时机 | 仅在 Skill 加载时执行，不在 Agent 对话过程中执行 |
| 用户感知 | 动态注入的命令及输出在 Skill 编辑器中可预览 |

---

## 5. 字符串替换系统

### 5.1 替换变量一览

SKILL.md 正文中可使用以下变量占位符，在 Skill 加载时由系统替换为实际值：

| 变量 | 说明 | 示例 |
|------|------|------|
| `$ARGUMENTS` | 所有传入参数的原始字符串 | `/deploy apps/web production` 中的 `apps/web production` |
| `$ARGUMENTS[0]` 或 `$1` | 第一个位置参数 | `apps/web` |
| `$ARGUMENTS[1]` 或 `$2` | 第二个位置参数 | `production` |
| `$name` | 命名参数（frontmatter `arguments` 中定义的名称） | `$project_dir` -> `apps/web` |
| `${ASTRO_SESSION_ID}` | 当前会话 ID | `conv-abc123` |
| `${ASTRO_SKILL_DIR}` | 当前 Skill 目录的绝对路径 | `/home/user/.astro/skills/deploy-vercel` |
| `${ASTRO_PROJECT_DIR}` | 当前工作区根目录 | `/home/user/my-project` |
| `${ASTRO_WORKSPACE_ID}` | 当前工作区 ID | `ws-abc123` |
| `${ASTRO_EFFORT}` | 当前推理努力级别 | `high` |
| `${ASTRO_MODEL}` | 当前使用的模型 | `claude-sonnet-4-20250514` |
| `${ASTRO_DEPTH}` | 当前 Agent 深度 (0=主 Agent) | `0` |

### 5.2 Rust 实现

```rust
// crates/agent-core/src/skills/substitution.rs

use std::collections::HashMap;

/// 替换上下文
pub struct SubstitutionContext {
    /// 原始参数字符串
    pub arguments_raw: String,
    /// 位置参数列表
    pub positional_args: Vec<String>,
    /// 命名参数
    pub named_args: HashMap<String, String>,
    /// 环境变量
    pub env_vars: HashMap<String, String>,
}

impl SubstitutionContext {
    /// 从调用参数和 Skill manifest 构建替换上下文
    pub fn from_invocation(
        raw_args: &str,
        skill_dir: &Path,
        project_dir: &Path,
        session_id: &str,
        workspace_id: &str,
        effort: &str,
        model: &str,
        depth: u8,
        arg_defs: &[ArgumentDef],
    ) -> Self {
        let positional: Vec<String> = raw_args
            .split_whitespace()
            .map(String::from)
            .collect();

        // 将位置参数映射到命名参数
        let mut named = HashMap::new();
        for (i, def) in arg_defs.iter().enumerate() {
            if let Some(val) = positional.get(i) {
                named.insert(def.name.clone(), val.clone());
            } else if let Some(default) = &def.default {
                named.insert(def.name.clone(), default.clone());
            }
        }

        let mut env = HashMap::new();
        env.insert("ASTRO_SESSION_ID".into(), session_id.into());
        env.insert("ASTRO_SKILL_DIR".into(), skill_dir.display().to_string());
        env.insert("ASTRO_PROJECT_DIR".into(), project_dir.display().to_string());
        env.insert("ASTRO_WORKSPACE_ID".into(), workspace_id.into());
        env.insert("ASTRO_EFFORT".into(), effort.into());
        env.insert("ASTRO_MODEL".into(), model.into());
        env.insert("ASTRO_DEPTH".into(), depth.to_string());

        Self {
            arguments_raw: raw_args.into(),
            positional_args: positional,
            named_args: named,
            env_vars: env,
        }
    }
}

/// 执行字符串替换
pub fn apply_substitutions(content: &str, ctx: &SubstitutionContext) -> String {
    let mut result = content.to_string();

    // $ARGUMENTS (全部参数)
    result = result.replace("$ARGUMENTS", &ctx.arguments_raw);

    // $ARGUMENTS[N] 和 $N (位置参数)
    for (i, arg) in ctx.positional_args.iter().enumerate() {
        result = result.replace(&format!("$ARGUMENTS[{}]", i), arg);
        result = result.replace(&format!("${}", i + 1), arg);
    }

    // $name (命名参数)
    for (name, value) in &ctx.named_args {
        result = result.replace(&format!("${}", name), value);
    }

    // ${ASTRO_*} (环境变量)
    for (key, value) in &ctx.env_vars {
        result = result.replace(&format!("${{{}}}", key), value);
    }

    result
}
```

### 5.3 处理顺序

替换操作在动态上下文注入**之后**执行，确保注入命令中也可以使用替换变量。完整处理流程：

```text
SKILL.md 原始正文
    |
    v
(1) 字符串替换 ($ARGUMENTS, $name, ${ASTRO_*})
    |
    v
(2) 动态上下文注入 (!`command`, ```! block```)
    |
    v
(3) 最终正文 -> 注入系统提示
```

---

## 6. 多级发现与加载

### 6.1 发现路径与优先级

系统在以下路径中扫描 Skill 目录，按优先级从高到低排列：

```text
优先级（高 -> 低）：

1. 子目录级  {project}/<subdir>/.astro/skills/<name>/SKILL.md
2. 工作区级  {project}/.astro/skills/<name>/SKILL.md
3. 全局级    ~/.astro/skills/<name>/SKILL.md
4. 插件级    ~/.astro/plugins/<plugin>/skills/<name>/SKILL.md
5. 内置级    <app-binary>/resources/builtin-skills/<name>/SKILL.md
```

同名 Skill 出现在多个层级时，高优先级层级覆盖低优先级层级。

### 6.2 冲突解决规则

```text
发现: ~/.astro/skills/deploy/SKILL.md              (全局)
发现: /project/.astro/skills/deploy/SKILL.md        (工作区)
发现: /project/apps/web/.astro/skills/deploy/SKILL.md (子目录)

解决策略:
|- 当前 cwd = /project/apps/web/src
|   -> 使用子目录级 deploy（最近的祖先目录）
|- 当前 cwd = /project/packages/lib
|   -> 使用工作区级 deploy（无子目录级匹配）
|- 全局级 deploy 被工作区级遮蔽，不生效
```

### 6.3 SkillLoader 实现

```rust
// crates/agent-core/src/skills/loader.rs

use notify::{Watcher, RecursiveMode, Event};

/// Skill 发现源
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillSource {
    /// 内置 Skill（随二进制分发）
    Builtin,
    /// 全局 Skill（~/.astro/skills/）
    Global,
    /// 工作区级 Skill（.astro/skills/）
    Workspace { workspace_id: String },
    /// 子目录级 Skill（monorepo 子目录）
    Subdirectory { relative_path: String },
    /// 插件 Skill
    Plugin { plugin_id: String },
    /// 进化引擎合成
    Synthesized,
    /// 市场安装
    Marketplace { package_id: String },
}

/// Skill 发现结果
pub struct DiscoveredSkill {
    pub manifest: SkillManifest,
    pub content: String,
    pub source: SkillSource,
    pub skill_dir: PathBuf,
    pub priority: u32,       // 数值越大优先级越高
}

pub struct SkillLoader {
    /// 内置 Skill 根目录
    builtin_root: PathBuf,
    /// 全局 Skill 根目录
    global_root: PathBuf,
    /// 工作区根目录
    workspace_root: Option<PathBuf>,
    /// 文件监视器
    watcher: Option<notify::RecommendedWatcher>,
}

impl SkillLoader {
    /// 扫描所有层级，返回去重后的 Skill 列表
    pub async fn discover_all(&self) -> anyhow::Result<Vec<DiscoveredSkill>> {
        let mut all: Vec<DiscoveredSkill> = Vec::new();

        // 按优先级从低到高扫描（后加入的覆盖先加入的）
        all.extend(self.scan_directory(&self.builtin_root, SkillSource::Builtin, 10).await?);
        all.extend(self.scan_directory(&self.global_root, SkillSource::Global, 30).await?);

        if let Some(ref ws_root) = self.workspace_root {
            // 插件级
            let plugin_dir = self.global_root.parent().unwrap().join("plugins");
            if plugin_dir.exists() {
                all.extend(self.scan_plugins(&plugin_dir).await?);
            }

            // 工作区级
            let ws_skills = ws_root.join(".astro/skills");
            all.extend(self.scan_directory(
                &ws_skills,
                SkillSource::Workspace { workspace_id: "default".into() },
                50,
            ).await?);

            // 子目录级（递归扫描 monorepo）
            all.extend(self.scan_subdirectories(ws_root).await?);
        }

        // 去重：同名 Skill 保留最高优先级
        let mut deduped: HashMap<String, DiscoveredSkill> = HashMap::new();
        for skill in all {
            let entry = deduped.entry(skill.manifest.name.clone()).or_insert(skill.clone());
            if skill.priority > entry.priority {
                *entry = skill;
            }
        }

        Ok(deduped.into_values().collect())
    }

    /// 扫描单个目录下的所有 Skill
    async fn scan_directory(
        &self,
        root: &Path,
        source: SkillSource,
        priority: u32,
    ) -> anyhow::Result<Vec<DiscoveredSkill>> {
        let mut skills = Vec::new();
        if !root.exists() { return Ok(skills); }

        let mut entries = tokio::fs::read_dir(root).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() { continue; }

            let skill_md = path.join("SKILL.md");
            if !skill_md.exists() { continue; }

            match self.load_skill_md(&skill_md, &path, source.clone(), priority).await {
                Ok(skill) => skills.push(skill),
                Err(e) => tracing::warn!("加载 Skill {:?} 失败: {}", skill_md, e),
            }
        }

        Ok(skills)
    }

    /// 解析单个 SKILL.md 文件
    async fn load_skill_md(
        &self,
        skill_md: &Path,
        skill_dir: &Path,
        source: SkillSource,
        priority: u32,
    ) -> anyhow::Result<DiscoveredSkill> {
        let raw = tokio::fs::read_to_string(skill_md).await?;
        let manifest = SkillManifest::parse_from_md(&raw)?;
        Ok(DiscoveredSkill { manifest, content: raw, source, skill_dir: skill_dir.into(), priority })
    }
}
```

### 6.4 实时变更检测

系统通过文件监视器监控所有 Skill 目录，任何 SKILL.md 的新增、修改、删除都即时生效：

```rust
// crates/agent-core/src/skills/watcher.rs

use notify::{Watcher, RecursiveMode, Event, EventKind};
use tokio::sync::mpsc;

pub enum SkillFileEvent {
    Created { path: PathBuf },
    Modified { path: PathBuf },
    Deleted { path: PathBuf },
}

pub struct SkillWatcher {
    _watcher: notify::RecommendedWatcher,
    event_rx: mpsc::Receiver<SkillFileEvent>,
}

impl SkillWatcher {
    pub fn new(watch_dirs: Vec<PathBuf>) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::channel(100);

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res {
                for path in &event.paths {
                    if path.file_name().map(|f| f == "SKILL.md").unwrap_or(false) {
                        let skill_event = match event.kind {
                            EventKind::Create(_) => SkillFileEvent::Created { path: path.clone() },
                            EventKind::Modify(_) => SkillFileEvent::Modified { path: path.clone() },
                            EventKind::Remove(_) => SkillFileEvent::Deleted { path: path.clone() },
                            _ => continue,
                        };
                        tx.blocking_send(skill_event).ok();
                    }
                }
            }
        })?;

        for dir in &watch_dirs {
            if dir.exists() {
                watcher.watch(dir, RecursiveMode::Recursive)?;
            }
        }

        Ok(Self { _watcher: watcher, event_rx: rx })
    }

    /// 后台消费文件事件，更新 SkillRegistry
    pub async fn run(mut self, registry: Arc<RwLock<SkillRegistry>>) {
        while let Some(event) = self.event_rx.recv().await {
            match event {
                SkillFileEvent::Created { path } | SkillFileEvent::Modified { path } => {
                    if let Ok(raw) = tokio::fs::read_to_string(&path).await {
                        if let Ok(manifest) = SkillManifest::parse_from_md(&raw) {
                            let mut reg = registry.write().await;
                            reg.update(manifest, &raw);
                            tracing::info!("Skill 热更新: {:?}", path);
                        }
                    }
                }
                SkillFileEvent::Deleted { path } => {
                    // 从路径推断 skill name
                    if let Some(name) = path.parent()
                        .and_then(|p| p.file_name())
                        .and_then(|f| f.to_str())
                    {
                        let mut reg = registry.write().await;
                        reg.retire_by_name(name);
                        tracing::info!("Skill 已移除: {}", name);
                    }
                }
            }
        }
    }
}
```

无需重启应用，文件系统变更在秒级内反映到 Agent 的可用 Skill 集合中。

---

## 7. 调用控制

### 7.1 调用模式矩阵

Skill 的调用由 `disable-model-invocation` 和 `user-invocable` 两个字段联合控制：

```text
                    user-invocable
                    true (默认)          false
                +-------------------+--------------------+
 disable-model  | 默认模式           | 仅模型调用          |
 -invocation    | 用户 /name YES    | 用户 /name NO      |
 false (默认)   | 模型自动触发 YES  | 模型自动触发 YES   |
                | 示例: translate    | 示例: bg-context    |
                +-------------------+--------------------+
                | 仅用户调用         | 完全禁用            |
 true           | 用户 /name YES    | 用户 /name NO      |
                | 模型自动触发 NO   | 模型自动触发 NO    |
                | 示例: /deploy      | （无实际用途）       |
                +-------------------+--------------------+
```

### 7.2 描述常驻与 BM25 召回

无论调用模式如何配置，所有 Skill 的 `description`（和 `when_to_use`）始终存在于系统提示中，作为模型匹配候选列表的一部分。这样即使模型不主动调用某 Skill，也能在用户提问时引导用户使用相关 Skill。

模型自动触发的流程依赖 BM25 语义召回：

```text
用户输入: "帮我把这个项目部署到 vercel"
    |
    v
BM25 检索（query = 用户输入）
    | 索引字段：name(x3) + description(x2) + trigger_patterns(x2) + content_body(x1)
    v
匹配结果:
    |- deploy-vercel  score=0.92  <- 超过 trigger_threshold(0.6)
    |- deploy-aws     score=0.41  <- 未达阈值，忽略
    |- code-review    score=0.12  <- 未达阈值，忽略
    |
    v
注入 deploy-vercel 的完整正文到系统提示
    |
    v
模型在完整 Skill 内容指导下执行部署任务
```

### 7.3 用户手动调用

用户通过 `/name` 斜杠命令直接调用 Skill：

```text
用户输入: /deploy-vercel apps/web production
                |            |         |
                |            +--- $1 / $project_dir
                |                      +--- $2 / $environment
                +--- Skill 名称
```

手动调用时跳过 BM25 匹配，直接加载 Skill 正文并执行字符串替换和动态注入。

### 7.4 调用流程全景

```text
用户消息到达
    |
    |- 以 / 开头？
    |    |
    |    |- YES -> 解析 Skill 名称和参数
    |    |         +- user-invocable == true?
    |    |              |- YES -> 加载 Skill 正文 -> 注入系统提示
    |    |              +- NO  -> 提示"此 Skill 不支持手动调用"
    |    |
    |    +- NO -> BM25 召回 top-k Skill
    |             +- 遍历匹配结果
    |                  +- score >= trigger_threshold
    |                     AND disable-model-invocation == false?
    |                       |- YES -> 加载 Skill 正文 -> 注入系统提示
    |                       +- NO  -> 仅保留 description 在系统提示中
    |
    v
Agent 执行（带有或不带有 Skill 上下文）
```

---

## 8. 内容生命周期

### 8.1 加载时机

Skill 内容的加载遵循"懒加载"策略：

| 阶段 | 加载内容 | 持久性 |
|------|---------|--------|
| 系统启动 | 所有 Skill 的 `name` + `description` + `when_to_use` | 常驻系统提示 |
| 用户消息到达 | BM25 召回匹配的 Skill 完整正文（含动态注入和替换） | 本次会话持久 |
| 用户 `/name` 调用 | 目标 Skill 完整正文 | 本次会话持久 |

### 8.2 会话内持久性

Skill 正文一旦加载，在整个会话期间保持可见。这意味着 Agent 在后续轮次中仍能参考之前加载的 Skill 内容。

### 8.3 上下文压缩行为

当对话历史超过 token 预算触发上下文压缩时，已加载的 Skill 内容受以下规则约束：

```rust
// crates/agent-core/src/skills/lifecycle.rs

/// Skill 内容在上下文压缩时的保留策略
pub struct SkillRetentionPolicy {
    /// 单个 Skill 的最大保留 token 数
    pub max_tokens_per_skill: usize,
    /// 所有 Skill 的总保留预算
    pub total_budget: usize,
}

impl Default for SkillRetentionPolicy {
    fn default() -> Self {
        Self {
            max_tokens_per_skill: 5_000,
            total_budget: 25_000,
        }
    }
}

/// 压缩时重新附加 Skill 内容
pub fn reattach_skills_after_compression(
    active_skills: &[LoadedSkill],
    policy: &SkillRetentionPolicy,
) -> Vec<String> {
    let mut budget_remaining = policy.total_budget;
    let mut reattached = Vec::new();

    // 按最近使用时间排序，优先保留最近引用的 Skill
    let mut sorted = active_skills.to_vec();
    sorted.sort_by(|a, b| b.last_referenced_at.cmp(&a.last_referenced_at));

    for skill in sorted {
        let token_count = estimate_tokens(&skill.content);
        let allowed = token_count.min(policy.max_tokens_per_skill).min(budget_remaining);

        if allowed == 0 { break; }

        let truncated = truncate_to_tokens(&skill.content, allowed);
        reattached.push(format!("## Skill: {}\n{}", skill.name, truncated));
        budget_remaining -= allowed;
    }

    reattached
}
```

### 8.4 去重机制

如果同一个 Skill 在会话中被多次调用且内容未变，系统不会重复注入完整正文，而是添加引用说明：

```text
[Skill deploy-vercel 已在本次会话中加载，内容与上次相同。
 参数已更新：$project_dir=apps/api, $environment=production]
```

---

## 9. Subagent 执行

### 9.1 `context: fork` 模式

当 Skill 在 frontmatter 中声明 `context: fork` 时，Skill 的正文将作为独立子 Agent 的任务提示执行，而非注入当前对话的系统提示。

```yaml
---
name: deep-research
description: 对给定主题进行多源深度研究
context: fork
agent: explore            # 子 Agent 类型
model: claude-sonnet-4-20250514
effort: high
---

对以下主题进行深度研究：$ARGUMENTS

## 研究要求
1. 搜索至少 3 个不同来源
2. 交叉验证关键事实
3. 生成结构化研究报告
```

### 9.2 执行流程

```text
用户调用 /deep-research "量子计算最新进展"
    |
    v
Skill Loader 解析 SKILL.md
    |
    |- context == "fork"?
    |    |
    |    +- YES -> 进入 Subagent 执行路径
    |
    v
(1) 字符串替换 + 动态注入 -> 生成最终正文
    |
    v
(2) 构建 SpawnConfig:
    |- goal = Skill 正文（替换后）
    |- role = agent 字段值 ("explore")
    |- model = model 字段值
    |- allowed_tools = allowed-tools 字段
    +- timeout = timeout_secs 字段
    |
    v
(3) Supervisor::spawn_child(config, parent_ctx)
    |   （详见 06-子Agent派生详细设计.md）
    |
    v
(4) 子 Agent 独立执行 round_loop
    |   |- 完全隔离的上下文
    |   |- 无法访问父 Agent 对话历史
    |   +- 使用 Skill 声明的工具集
    |
    v
(5) 子 Agent 完成 -> SubAgentResult
    |
    v
(6) 结果摘要注入父 Agent 对话
```

### 9.3 与 Supervisor 的集成

```rust
// crates/agent-runtime/src/skills/subagent.rs

use crate::supervisor::{Supervisor, SpawnConfig, SubAgentResult};

/// 以 Subagent 模式执行 Skill
pub async fn execute_skill_as_subagent(
    skill: &LoadedSkill,
    raw_args: &str,
    parent_ctx: &AgentContext,
    supervisor: &Supervisor,
) -> anyhow::Result<SubAgentResult> {
    // 构建 SpawnConfig
    let config = SpawnConfig {
        goal: skill.processed_content.clone(),  // 替换+注入后的正文
        context: format!(
            "执行 Skill: {}\n描述: {}",
            skill.manifest.name, skill.manifest.description
        ),
        role: skill.manifest.agent_type
            .as_deref()
            .unwrap_or("worker")
            .to_string(),
        model_override: skill.manifest.model.clone(),
        allowed_tools: skill.manifest.allowed_tools.clone(),
        timeout_secs: skill.manifest.timeout_secs.unwrap_or(300),
        system_prompt_override: Some(skill.processed_content.clone()),
        ..Default::default()
    };

    let child_id = supervisor.spawn_child(config, parent_ctx).await?;
    supervisor.join_one(child_id).await
        .map_err(|e| anyhow::anyhow!("Subagent 执行失败: {}", e))
}
```

### 9.4 子 Agent 类型映射

| `agent` 字段值 | 子 Agent 类型 | 特点 |
|----------------|--------------|------|
| `explore` | 只读搜索 Agent | 仅具备读取和搜索工具 |
| `plan` | 规划 Agent | 不执行修改，输出实施计划 |
| `general-purpose` | 通用 Agent | 继承父 Agent 工具集（减去屏蔽列表） |
| 自定义名称 | 自定义 Agent 类型 | 由 Agent 类型注册表中的定义决定 |

---

## 10. Skill 作用域 Hooks

### 10.1 概念

Skill 可在 frontmatter 中声明仅在该 Skill 激活期间生效的 Hooks。这些 Hooks 在 Skill 加载时注册到 HookRegistry，在 Skill 卸载时注销。

### 10.2 frontmatter 声明

```yaml
hooks:
  before_tool_execute:
    command: "scripts/lint-check.sh $TOOL_ARGS"
    on_failure: abort
    timeout_ms: 5000
    tool_filter: file_write        # 仅在调用 file_write 时触发
  after_tool_execute:
    command: "scripts/post-validate.sh"
    on_failure: continue
  before_llm_call:
    command: "scripts/inject-context.sh"
    on_failure: continue
```

### 10.3 生命周期与 HookRegistry 集成

```rust
// crates/agent-runtime/src/skills/hooks_bridge.rs

use crate::hooks::{HookRegistry, HookSource, ShellHook, ShellHookConfig, HookEvent};

/// 注册 Skill 作用域 Hooks
pub fn register_skill_hooks(
    registry: &HookRegistry,
    skill_name: &str,
    skill_dir: &Path,
    hooks: &HashMap<String, SkillHookDef>,
) -> Vec<String> {
    let mut registered_names = Vec::new();

    for (event_name, def) in hooks {
        let event = match event_name.as_str() {
            "before_tool_execute" => HookEvent::BeforeToolExecute,
            "after_tool_execute" => HookEvent::AfterToolExecute,
            "before_llm_call" => HookEvent::BeforeLlmCall,
            "after_llm_call" => HookEvent::AfterLlmCall,
            "on_user_message" => HookEvent::OnUserMessage,
            _ => {
                tracing::warn!("Skill {} 声明了未知 Hook 事件: {}", skill_name, event_name);
                continue;
            }
        };

        let hook_name = format!("skill:{}:{}", skill_name, event_name);

        // 将 Skill 目录作为命令的 cwd
        let command = def.command.replace(
            "scripts/",
            &format!("{}/scripts/", skill_dir.display()),
        );

        let config = ShellHookConfig {
            name: hook_name.clone(),
            event,
            tool_filter: def.tool_filter.clone(),
            command,
            timeout_ms: def.timeout_ms.unwrap_or(5_000),
            on_failure: def.on_failure.clone().unwrap_or("continue".into()).into(),
            priority: 450,   // Skill Hooks 优先级固定在 450
            enabled: true,
        };

        let hook = Arc::new(ShellHook::from_config(config));
        if registry.register(hook, HookSource::Runtime).is_ok() {
            registered_names.push(hook_name);
        }
    }

    registered_names
}

/// 注销 Skill 作用域 Hooks（Skill 卸载时调用）
pub fn unregister_skill_hooks(
    registry: &HookRegistry,
    hook_names: &[String],
) {
    for name in hook_names {
        registry.unregister(name).ok();
    }
}
```

### 10.4 与全局 Hooks 的关系

Skill 作用域 Hooks 与全局 Hooks（`hooks.toml` 定义）共存于同一个 `HookRegistry` 中，按统一的优先级机制执行。Skill Hooks 的优先级固定为 450（在用户配置级 400-499 范围内），确保：

- 系统级（0-99）和安全级（100-199）Hook 始终先于 Skill Hook 执行
- Skill Hook 在用户全局 Hook（400-449）之后执行
- 插件 Hook（500+）在 Skill Hook 之后执行

---

## 11. 工具权限控制

### 11.1 per-Skill 权限作用域

Skill 可通过 `allowed-tools` 和 `disallowed-tools` 声明其激活期间的工具权限变更：

```yaml
allowed-tools: shell_exec file_write http_request
disallowed-tools: git_push memory_write
```

### 11.2 权限计算规则

```text
Skill 激活时的有效工具集 =
    (Agent 当前工具集 + allowed-tools) - disallowed-tools

约束：
    allowed-tools 不可包含 Agent 本身未获授权的工具
    即 allowed-tools 必须是 Agent 全量可用工具集的子集
```

### 11.3 Rust 实现

```rust
// crates/agent-core/src/skills/permissions.rs

/// 计算 Skill 激活后的有效工具集
pub fn compute_skill_tool_set(
    agent_tools: &ToolRegistry,
    agent_potential_tools: &HashSet<String>,  // Agent 的全量可用工具（含未激活的）
    allowed: &Option<Vec<String>>,
    disallowed: &Option<Vec<String>>,
) -> ToolPermissionDelta {
    let mut grants = Vec::new();
    let mut revokes = Vec::new();

    if let Some(allowed) = allowed {
        for tool_name in allowed {
            // 只有 Agent 本身有权使用的工具才能授予
            if agent_potential_tools.contains(tool_name) {
                if !agent_tools.has(tool_name) {
                    grants.push(tool_name.clone());
                }
            } else {
                tracing::warn!(
                    "Skill 声明的 allowed-tools 包含 Agent 未授权的工具: {}，已忽略",
                    tool_name
                );
            }
        }
    }

    if let Some(disallowed) = disallowed {
        for tool_name in disallowed {
            revokes.push(tool_name.clone());
        }
    }

    ToolPermissionDelta { grants, revokes }
}

pub struct ToolPermissionDelta {
    pub grants: Vec<String>,    // 额外启用的工具
    pub revokes: Vec<String>,   // 屏蔽的工具
}
```

### 11.4 权限恢复

Skill 的权限变更仅在 Skill 激活期间生效。当 Skill 执行完毕或被手动卸载时，工具权限自动恢复到 Skill 加载前的状态。

---

## 12. Skill Overrides

### 12.1 概念

Skill Overrides 允许用户通过 `settings.json` 控制 Skill 的可见性和调用模式，而无需编辑 Skill 的 SKILL.md 文件。这对于：
- 临时禁用某个 Skill（不删除文件）
- 将自动触发的 Skill 改为仅用户调用
- 在特定工作区覆盖全局 Skill 行为

### 12.2 配置格式

```json
// ~/.astro/settings.json 或 {workspace}/.astro/settings.json
{
  "skillOverrides": {
    "deploy-vercel": "off",
    "legacy-context": "name-only",
    "auto-format": "user-invocable-only",
    "translate": "on"
  }
}
```

### 12.3 Override 状态

| 状态值 | 效果 |
|--------|------|
| `"on"` | 默认行为，等同于不配置 override |
| `"name-only"` | 仅 `name` + `description` 出现在系统提示，不加载正文、不可调用 |
| `"user-invocable-only"` | 等同于 `disable-model-invocation: true`，仅用户可调用 |
| `"off"` | 完全禁用，从系统提示中移除，不可被发现 |

### 12.4 Rust 实现

```rust
// crates/agent-core/src/skills/overrides.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillOverrideState {
    On,
    NameOnly,
    UserInvocableOnly,
    Off,
}

/// 加载 Skill Overrides（合并全局和工作区级配置）
pub fn load_skill_overrides(
    global_settings: &Path,
    workspace_settings: Option<&Path>,
) -> HashMap<String, SkillOverrideState> {
    let mut overrides = HashMap::new();

    // 全局设置
    if let Ok(content) = std::fs::read_to_string(global_settings) {
        if let Ok(settings) = serde_json::from_str::<SettingsFile>(&content) {
            overrides.extend(settings.skill_overrides);
        }
    }

    // 工作区设置覆盖全局
    if let Some(ws_path) = workspace_settings {
        if let Ok(content) = std::fs::read_to_string(ws_path) {
            if let Ok(settings) = serde_json::from_str::<SettingsFile>(&content) {
                overrides.extend(settings.skill_overrides);
            }
        }
    }

    overrides
}

/// 应用 Override 到 Skill 的调用控制字段
pub fn apply_override(
    manifest: &mut SkillManifest,
    state: &SkillOverrideState,
) {
    match state {
        SkillOverrideState::On => {
            // 不修改，使用 SKILL.md 原始设置
        }
        SkillOverrideState::NameOnly => {
            manifest.disable_model_invocation = true;
            manifest.user_invocable = false;
            manifest.override_state = Some(SkillOverrideState::NameOnly);
        }
        SkillOverrideState::UserInvocableOnly => {
            manifest.disable_model_invocation = true;
            manifest.user_invocable = true;
            manifest.override_state = Some(SkillOverrideState::UserInvocableOnly);
        }
        SkillOverrideState::Off => {
            manifest.override_state = Some(SkillOverrideState::Off);
            // SkillRegistry 在索引时跳过 Off 状态的 Skill
        }
    }
}

#[derive(Debug, Deserialize)]
struct SettingsFile {
    #[serde(default, rename = "skillOverrides")]
    skill_overrides: HashMap<String, SkillOverrideState>,
}
```

---

## 13. SkillManifest 数据模型

### 13.1 完整 Rust 结构

```rust
// crates/agent-core/src/skills/manifest.rs

use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Skill 元数据 -- 从 SKILL.md frontmatter 解析
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    // -- 标识 --
    pub id: String,                             // UUID，首次索引时生成
    pub name: String,                           // SKILL.md name 字段
    pub description: String,                    // 常驻系统提示（限 120 token）
    pub when_to_use: Option<String>,            // 使用场景补充描述

    // -- 调用控制 --
    pub disable_model_invocation: bool,         // 禁止模型自动触发
    pub user_invocable: bool,                   // 是否出现在 / 菜单
    pub override_state: Option<SkillOverrideState>, // settings.json 覆盖状态

    // -- 工具权限 --
    pub allowed_tools: Option<Vec<String>>,     // 激活时额外授予的工具
    pub disallowed_tools: Option<Vec<String>>,  // 激活时屏蔽的工具
    pub requires_tools: Vec<String>,            // 声明依赖的工具（预检用）

    // -- 执行控制 --
    pub model: Option<String>,                  // 模型覆盖
    pub effort: Option<String>,                 // 推理努力级别
    pub context: Option<String>,                // "fork" = Subagent 执行
    pub agent_type: Option<String>,             // Subagent 类型
    pub shell: Option<String>,                  // 脚本 shell

    // -- 参数 --
    pub arguments: Vec<ArgumentDef>,            // 命名位置参数
    pub argument_hint: Option<String>,          // 自动补全提示
    pub parameters: HashMap<String, ParameterDef>, // Schema 校验用参数

    // -- 作用域 --
    pub paths: Vec<String>,                     // glob 限定激活路径

    // -- Hooks --
    pub hooks: HashMap<String, SkillHookDef>,   // Skill 作用域 Hooks

    // -- Astro 扩展 --
    pub version: String,                        // semver
    pub origin: SkillOrigin,                    // 来源
    pub status: SkillStatus,                    // 生命周期状态
    pub trigger_patterns: Vec<String>,          // BM25 触发关键词
    pub trigger_threshold: f32,                 // BM25 阈值
    pub risk_level: Option<String>,             // 风险等级 L0-L3
    pub exec_backend: Option<String>,           // 执行后端
    pub timeout_secs: Option<u32>,              // 超时

    pub tags: Vec<String>,                      // 分类标签
    pub parent_skill: Option<String>,           // 进化链前驱
    pub deprecates: Option<String>,             // 替代的旧 Skill

    pub test_cases: Vec<TestCaseDef>,           // 内置测试用例

    // -- 系统元数据 --
    pub workspace_id: Option<String>,           // None = 全局
    pub content_hash: String,                   // SHA-256(正文)
    pub entry_file: PathBuf,                    // SKILL.md 绝对路径
    pub skill_dir: PathBuf,                     // Skill 目录绝对路径
    pub source: SkillSource,                    // 发现来源
    pub created_at: i64,
    pub updated_at: i64,
}

/// Skill 来源（如何产生）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillOrigin {
    Builtin,         // 随二进制分发
    Synthesized,     // 进化引擎自动生成
    Imported,        // 用户手动创建
    Marketplace,     // 从市场安装
}

/// Skill 生命周期状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillStatus {
    Draft,           // 草稿
    Ready,           // 本地就绪
    Published,       // 已发布
    Deprecated,      // 已废弃
    Evolving,        // 进化引擎正在处理
}

/// 命名位置参数定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgumentDef {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
    pub default: Option<String>,
}

/// Schema 参数定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterDef {
    #[serde(rename = "type")]
    pub param_type: String,
    pub description: Option<String>,
    pub default: Option<serde_json::Value>,
    pub required: bool,
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<serde_json::Value>>,
}

/// Skill 作用域 Hook 定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillHookDef {
    pub command: String,
    pub on_failure: Option<String>,   // abort | continue | skip
    pub timeout_ms: Option<u64>,
    pub tool_filter: Option<String>,
}

/// 内置测试用例定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCaseDef {
    pub name: String,
    pub params: serde_json::Value,
    pub expect_exit_code: Option<i32>,
    pub expect_output_contains: Option<String>,
    pub expect_output_not_contains: Option<String>,
}
```

### 13.2 SkillSource 与 SkillOrigin 的区别

`SkillSource` 描述 Skill 是**在哪里被发现**的（文件系统位置），`SkillOrigin` 描述 Skill 是**如何产生**的（创建方式）。两者正交：

```text
SkillSource::Global + SkillOrigin::Synthesized
  -> 进化引擎在全局目录下合成的 Skill

SkillSource::Workspace + SkillOrigin::Marketplace
  -> 从市场安装到特定工作区的 Skill
```

---

## 14. SkillRegistry

### 14.1 核心结构

```rust
// crates/agent-core/src/skills/registry.rs

use tantivy::{Index, IndexWriter, schema::*, query::QueryParser, collector::TopDocs};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Skill 搜索结果
pub struct SkillSearchResult {
    pub manifest: SkillManifest,
    pub score: f32,
    pub snippet: String,
}

/// Skill 注册表：BM25 索引 + 内存 manifest 缓存
pub struct SkillRegistry {
    /// BM25 倒排索引（tantivy crate）
    index: Index,
    /// 索引写入器
    writer: IndexWriter,
    /// 内存 manifest 缓存：name -> SkillManifest
    manifests: HashMap<String, SkillManifest>,
    /// 完整正文缓存：name -> content
    contents: HashMap<String, String>,
    /// Override 配置
    overrides: HashMap<String, SkillOverrideState>,
    /// Schema 字段句柄
    fields: SkillIndexFields,
}

struct SkillIndexFields {
    id: Field,
    name: Field,           // 权重 x3
    description: Field,    // 权重 x2
    triggers: Field,       // 权重 x2
    when_to_use: Field,    // 权重 x2
    tags: Field,           // 权重 x1
    content: Field,        // 权重 x1
}
```

### 14.2 BM25 检索

```rust
impl SkillRegistry {
    /// 按 BM25 分数返回 top-k 相关 Skill
    pub fn search(&self, query: &str, top_k: usize) -> Vec<SkillSearchResult> {
        let reader = self.index.reader().unwrap();
        let searcher = reader.searcher();

        // 多字段加权查询
        let query_parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.name,
                self.fields.description,
                self.fields.triggers,
                self.fields.when_to_use,
                self.fields.tags,
                self.fields.content,
            ],
        );

        // 设置字段权重
        query_parser.set_field_boost(self.fields.name, 3.0);
        query_parser.set_field_boost(self.fields.description, 2.0);
        query_parser.set_field_boost(self.fields.triggers, 2.0);
        query_parser.set_field_boost(self.fields.when_to_use, 2.0);

        let parsed = match query_parser.parse_query(query) {
            Ok(q) => q,
            Err(_) => return vec![],
        };

        let top_docs = searcher.search(&parsed, &TopDocs::with_limit(top_k))
            .unwrap_or_default();

        top_docs.into_iter()
            .filter_map(|(score, addr)| {
                let doc = searcher.doc(addr).ok()?;
                let name = doc.get_first(self.fields.name)?.as_text()?;
                let manifest = self.manifests.get(name)?.clone();

                // 检查 Override 状态
                if let Some(SkillOverrideState::Off) = self.overrides.get(name) {
                    return None;
                }

                // 检查分数阈值
                if score < manifest.trigger_threshold {
                    return None;
                }

                // 检查模型调用权限
                if manifest.disable_model_invocation {
                    return None;
                }

                let snippet = doc.get_first(self.fields.description)
                    .and_then(|f| f.as_text())
                    .unwrap_or("")
                    .to_string();

                Some(SkillSearchResult { manifest, score, snippet })
            })
            .collect()
    }

    /// 热更新：新增或更新 Skill
    pub fn update(&mut self, manifest: SkillManifest, content: &str) {
        // 先删除旧版本（如存在）
        self.retire_by_name(&manifest.name);

        // 添加到 tantivy 索引
        let mut doc = Document::new();
        doc.add_text(self.fields.id, &manifest.id);
        doc.add_text(self.fields.name, &manifest.name);
        doc.add_text(self.fields.description, &manifest.description);
        doc.add_text(self.fields.triggers, &manifest.trigger_patterns.join(" "));
        doc.add_text(self.fields.when_to_use,
            manifest.when_to_use.as_deref().unwrap_or(""));
        doc.add_text(self.fields.tags, &manifest.tags.join(" "));
        doc.add_text(self.fields.content, content);

        self.writer.add_document(doc).ok();
        self.writer.commit().ok();

        // 更新内存缓存
        self.manifests.insert(manifest.name.clone(), manifest);
        self.contents.insert(manifest.name.clone(), content.to_string());
    }

    /// 移除 Skill
    pub fn retire_by_name(&mut self, name: &str) {
        let term = Term::from_field_text(self.fields.name, name);
        self.writer.delete_term(term);
        self.writer.commit().ok();
        self.manifests.remove(name);
        self.contents.remove(name);
    }

    /// 获取 Skill 正文（供注入系统提示）
    pub fn get_content(&self, name: &str) -> Option<&str> {
        self.contents.get(name).map(|s| s.as_str())
    }

    /// 生成所有 Skill 的描述摘要（常驻系统提示）
    pub fn generate_skill_catalog(&self) -> String {
        let mut catalog = String::from("# 可用 Skills\n\n");

        for (name, manifest) in &self.manifests {
            // 跳过 Off 状态的 Skill
            if matches!(self.overrides.get(name), Some(SkillOverrideState::Off)) {
                continue;
            }

            catalog.push_str(&format!("- **{}**: {}", name, manifest.description));

            if let Some(ref when) = manifest.when_to_use {
                catalog.push_str(&format!(" {}", when));
            }

            if manifest.user_invocable && !manifest.disable_model_invocation {
                catalog.push_str(" (可通过 /name 调用)");
            } else if manifest.user_invocable {
                catalog.push_str(" (仅 /name 调用)");
            }

            catalog.push('\n');
        }

        catalog
    }

    /// 刷新 Override 配置
    pub fn refresh_overrides(&mut self, overrides: HashMap<String, SkillOverrideState>) {
        self.overrides = overrides;
        // 对每个有 override 的 manifest 应用覆盖
        for (name, state) in &self.overrides {
            if let Some(manifest) = self.manifests.get_mut(name) {
                apply_override(manifest, state);
            }
        }
    }
}
```

### 14.3 召回注入流程

```rust
/// 对话前置处理：根据用户输入召回相关 Skill 并注入系统提示
pub async fn inject_skills(
    user_input: &str,
    registry: &SkillRegistry,
    system_prompt: &mut String,
    active_skills: &mut Vec<LoadedSkill>,
    injection_config: &ContextInjectionConfig,
    substitution_ctx: &SubstitutionContext,
) {
    let results = registry.search(user_input, 5);
    if results.is_empty() { return; }

    system_prompt.push_str("\n\n## 已激活的 Skills\n");

    for result in results {
        let name = &result.manifest.name;

        // 检查是否已加载（去重）
        if active_skills.iter().any(|s| s.name == *name) {
            system_prompt.push_str(&format!(
                "\n[Skill {} 已在本次会话中加载]\n", name
            ));
            continue;
        }

        // 加载正文
        if let Some(raw_content) = registry.get_content(name) {
            // 提取 frontmatter 之后的正文部分
            let body = extract_body(raw_content);

            // 字符串替换
            let substituted = apply_substitutions(&body, substitution_ctx);

            // 动态上下文注入
            let processed = process_dynamic_injection(&substituted, injection_config)
                .await
                .unwrap_or(substituted);

            system_prompt.push_str(&format!(
                "\n### Skill: {}\n{}\n", name, processed
            ));

            active_skills.push(LoadedSkill {
                name: name.clone(),
                content: processed,
                manifest: result.manifest.clone(),
                last_referenced_at: now_ms(),
            });
        }
    }
}
```

---

## 15. 存储与版本管理

### 15.1 DDL：skills 表

```sql
CREATE TABLE IF NOT EXISTS skills (
    id                TEXT PRIMARY KEY,
    workspace_id      TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    description       TEXT NOT NULL,
    when_to_use       TEXT,

    -- 调用控制
    disable_model_invocation  INTEGER NOT NULL DEFAULT 0,
    user_invocable            INTEGER NOT NULL DEFAULT 1,

    -- 执行控制
    model             TEXT,
    effort            TEXT,
    context           TEXT,                   -- "fork" | NULL
    agent_type        TEXT,
    shell             TEXT DEFAULT 'bash',

    -- Astro 扩展
    version           TEXT NOT NULL DEFAULT '1.0.0',
    origin            TEXT NOT NULL DEFAULT 'imported',
    status            TEXT NOT NULL DEFAULT 'draft',
    risk_level        TEXT DEFAULT 'L0',
    exec_backend      TEXT DEFAULT 'local',
    timeout_secs      INTEGER DEFAULT 300,

    -- 索引与检索
    trigger_patterns  TEXT,                   -- JSON array
    trigger_threshold REAL DEFAULT 0.5,
    tags              TEXT,                   -- JSON array
    content_hash      TEXT NOT NULL,          -- SHA-256(正文)

    -- 进化链
    parent_skill      TEXT,
    deprecates        TEXT,

    -- 文件系统
    entry_file        TEXT NOT NULL,
    skill_dir         TEXT NOT NULL,
    source            TEXT NOT NULL DEFAULT 'workspace',

    -- 时间戳
    created_at        INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    updated_at        INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_skills_name_ws
    ON skills(name, workspace_id);
CREATE INDEX IF NOT EXISTS idx_skills_status
    ON skills(status, workspace_id);
CREATE INDEX IF NOT EXISTS idx_skills_origin
    ON skills(origin);
```

### 15.2 DDL：skill_versions 表

```sql
CREATE TABLE IF NOT EXISTS skill_versions (
    id           TEXT PRIMARY KEY,
    skill_name   TEXT NOT NULL,
    workspace_id TEXT,
    version      TEXT NOT NULL,           -- semver
    content      TEXT NOT NULL,           -- 该版本的完整 SKILL.md 内容
    content_hash TEXT NOT NULL,           -- SHA-256
    changelog    TEXT,                    -- 与上一版本的差异说明
    breaking     INTEGER NOT NULL DEFAULT 0,  -- 是否包含破坏性变更
    created_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    created_by   TEXT NOT NULL            -- 'user' | 'evolution_engine' | 'marketplace'
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_skill_versions_name_ver
    ON skill_versions(skill_name, workspace_id, version);
CREATE INDEX IF NOT EXISTS idx_skill_versions_name
    ON skill_versions(skill_name, workspace_id, created_at DESC);
```

### 15.3 SkillRepo

```rust
// crates/agent-core/src/skills/repo.rs

pub struct SkillRepo(SqlitePool);

impl SkillRepo {
    /// 列出指定工作区的所有活跃 Skill（含全局）
    pub async fn list_active(&self, workspace_id: Option<&str>) -> Vec<SkillManifest> {
        sqlx::query_as!(
            SkillManifest,
            "SELECT * FROM skills
             WHERE status NOT IN ('deprecated')
               AND (workspace_id IS NULL OR workspace_id = ?)",
            workspace_id
        )
        .fetch_all(&self.0)
        .await
        .unwrap_or_default()
    }

    /// 插入新 Skill
    pub async fn insert(&self, manifest: &SkillManifest) -> anyhow::Result<()> {
        sqlx::query!(
            "INSERT INTO skills (id, workspace_id, name, description, when_to_use,
             disable_model_invocation, user_invocable, model, effort, context, agent_type,
             shell, version, origin, status, risk_level, exec_backend, timeout_secs,
             trigger_patterns, trigger_threshold, tags, content_hash,
             parent_skill, deprecates, entry_file, skill_dir, source)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            manifest.id, manifest.workspace_id, manifest.name, manifest.description,
            manifest.when_to_use, manifest.disable_model_invocation as i32,
            manifest.user_invocable as i32, manifest.model, manifest.effort,
            manifest.context, manifest.agent_type, manifest.shell,
            manifest.version, format!("{:?}", manifest.origin),
            format!("{:?}", manifest.status), manifest.risk_level,
            manifest.exec_backend, manifest.timeout_secs,
            serde_json::to_string(&manifest.trigger_patterns).ok(),
            manifest.trigger_threshold,
            serde_json::to_string(&manifest.tags).ok(),
            manifest.content_hash, manifest.parent_skill, manifest.deprecates,
            manifest.entry_file.display().to_string(),
            manifest.skill_dir.display().to_string(),
            format!("{:?}", manifest.source),
        )
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// 退役 Skill
    pub async fn retire(&self, id: &str, approved_by: &str) -> anyhow::Result<()> {
        sqlx::query!(
            "UPDATE skills SET status = 'deprecated', updated_at = ? WHERE id = ?",
            now_ms(), id
        )
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// 创建新版本（旧版本自动归档到 skill_versions）
    pub async fn create_next_version(
        &self,
        old_name: &str,
        new_manifest: &SkillManifest,
        new_content: &str,
        changelog: &str,
    ) -> anyhow::Result<()> {
        let mut tx = self.0.begin().await?;

        // 归档旧版本到 skill_versions
        let old = sqlx::query!(
            "SELECT version, content_hash FROM skills WHERE name = ? AND workspace_id IS ?",
            old_name, new_manifest.workspace_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(old) = old {
            let old_content = tokio::fs::read_to_string(
                &new_manifest.entry_file
            ).await.unwrap_or_default();

            sqlx::query!(
                "INSERT INTO skill_versions (id, skill_name, workspace_id, version,
                 content, content_hash, changelog, created_by)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                uuid::Uuid::new_v4().to_string(),
                old_name,
                new_manifest.workspace_id,
                old.version,
                old_content,
                old.content_hash,
                changelog,
                "system",
            )
            .execute(&mut *tx)
            .await?;
        }

        // 更新 skills 表为新版本
        sqlx::query!(
            "UPDATE skills SET version = ?, content_hash = ?, updated_at = ?
             WHERE name = ? AND workspace_id IS ?",
            new_manifest.version, new_manifest.content_hash, now_ms(),
            old_name, new_manifest.workspace_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}
```

### 15.4 版本号规则（semver）

| 变更类型 | 版本升级 | 破坏性 | 示例 |
|---------|---------|--------|------|
| 修复 Prompt 措辞 | patch | 否 | 1.0.0 -> 1.0.1 |
| 新增可选参数 | minor | 否 | 1.0.0 -> 1.1.0 |
| 新增必填参数 | major | 是 | 1.0.0 -> 2.0.0 |
| 改变行为语义 | major | 是 | 1.0.0 -> 2.0.0 |
| 进化引擎优化 | patch/minor | 自动判断 | -- |

### 15.5 向后兼容检查

```rust
/// 检测 Skill 更新中的破坏性变更
pub fn check_breaking_changes(old: &SkillManifest, new: &SkillManifest) -> Vec<BreakingChange> {
    let mut changes = Vec::new();

    // 1. 删除已有的必填参数
    for arg in &old.arguments {
        if arg.required && !new.arguments.iter().any(|a| a.name == arg.name) {
            changes.push(BreakingChange::RemovedRequiredArg(arg.name.clone()));
        }
    }

    // 2. 新增必填参数（无默认值）
    for arg in &new.arguments {
        if arg.required && arg.default.is_none()
            && !old.arguments.iter().any(|a| a.name == arg.name)
        {
            changes.push(BreakingChange::AddedRequiredArg(arg.name.clone()));
        }
    }

    // 3. 风险等级升高
    if risk_level_ord(old.risk_level.as_deref()) < risk_level_ord(new.risk_level.as_deref()) {
        changes.push(BreakingChange::RiskLevelIncreased);
    }

    // 4. 执行后端变更
    if old.exec_backend != new.exec_backend {
        changes.push(BreakingChange::ExecBackendChanged);
    }

    // 5. context 从 None 变为 fork（行为语义变更）
    if old.context.is_none() && new.context.as_deref() == Some("fork") {
        changes.push(BreakingChange::ContextModeChanged);
    }

    changes
}

#[derive(Debug, Clone)]
pub enum BreakingChange {
    RemovedRequiredArg(String),
    AddedRequiredArg(String),
    RiskLevelIncreased,
    ExecBackendChanged,
    ContextModeChanged,
}
```

---

## 16. 进化引擎集成

> 进化引擎的完整设计详见 [05-自我进化引擎详细设计.md](../_v0.3规划/05-自我进化引擎详细设计.md)。本节仅描述 Skills 系统与 EvolutionEngine 的集成接口。

### 16.1 集成架构

```text
+--------------------------------------------------------------+
|                     EvolutionEngine                          |
|                                                              |
|  +------------+   +--------------+   +------------------+   |
|  |  Trigger    |-->|   Strategy   |-->|  SkillSynthesizer |   |
|  |  Detector   |   |  (C / R / R) |   |  (LLM 合成)      |   |
|  +------------+   +--------------+   +--------+---------+   |
|                                                |             |
|                                     +----------v---------+   |
|                                     | ABTestRunner       |   |
|                                     | (候选版本对比)     |   |
|                                     +----------+---------+   |
|                                                |             |
+------------------------------------------------+-------------+
                                                 |
                  +--------------Skills 系统------v------------+
                  |                                            |
                  |  SkillRepo::insert()        -> skills 表   |
                  |  SkillRepo::create_next_version()          |
                  |  SkillRegistry::update()    -> BM25 索引   |
                  |  SKILL.md 文件写入          -> 文件系统     |
                  +--------------------------------------------+
```

### 16.2 三种进化动作

| 动作 | 触发条件 | Skills 系统行为 |
|------|---------|----------------|
| **Create** | 3+ 次相似成功任务且无匹配 Skill | 合成 SKILL.md -> `SkillRepo::insert()` -> status=draft |
| **Refine** | Skill 最近 5 次成功率 < 70% | 生成候选版本 -> A/B 测试 -> `SkillRepo::create_next_version()` |
| **Retire** | 连续 10 次召回未使用，或 30 天未使用 | `SkillRepo::retire()` -> status=deprecated |

### 16.3 进化结果写入

进化引擎产生的新 Skill 或更新版本最终通过标准的 SkillRepo 和 SkillRegistry 接口写入：

```rust
// EvolutionEngine 调用 Skills 系统的接口

impl EvolutionEngine {
    /// Create: 注册全新 Skill
    async fn register_synthesized_skill(
        &self,
        result: SynthesizedSkill,
        workspace_id: &str,
    ) -> anyhow::Result<()> {
        // 1. 写入 SKILL.md 文件（在工作区 skills 目录下创建 Skill 目录）
        let skill_dir = PathBuf::from(format!(
            "{}/.astro/workspaces/{}/skills/{}",
            dirs::home_dir().unwrap().display(),
            workspace_id,
            result.manifest.name,
        ));
        tokio::fs::create_dir_all(&skill_dir).await?;
        let file_path = skill_dir.join("SKILL.md");
        tokio::fs::write(&file_path, &result.content).await?;

        // 2. 写入 skills 表
        let mut manifest = result.manifest;
        manifest.entry_file = file_path;
        manifest.skill_dir = skill_dir;
        manifest.status = SkillStatus::Draft;
        manifest.origin = SkillOrigin::Synthesized;
        self.skill_repo.insert(&manifest).await?;

        // 3. 热更新 BM25 索引
        let mut registry = self.registry.write().await;
        registry.update(manifest, &result.content);

        Ok(())
    }

    /// Refine: 更新现有 Skill
    async fn apply_refinement(
        &self,
        result: RefineResult,
        workspace_id: &str,
    ) -> anyhow::Result<()> {
        let old = self.skill_repo.get(&result.skill_id).await?;

        // 1. 写入文件
        tokio::fs::write(&old.entry_file, &result.new_content).await?;

        // 2. 创建新版本
        let new_manifest = SkillManifest {
            version: semver_bump(&old.version, &result),
            content_hash: sha256(&result.new_content),
            updated_at: now_ms(),
            ..old.clone()
        };
        self.skill_repo.create_next_version(
            &old.name, &new_manifest, &result.new_content,
            &format!("进化引擎精化：策略={}, 改善={:.1}%",
                result.winner_label, result.improvement * 100.0),
        ).await?;

        // 3. 更新 BM25 索引
        let mut registry = self.registry.write().await;
        registry.update(new_manifest, &result.new_content);

        Ok(())
    }

    /// Retire: 废弃 Skill
    async fn retire_skill(&self, skill_id: &str) -> anyhow::Result<()> {
        self.skill_repo.retire(skill_id, "evolution_engine").await?;
        let mut registry = self.registry.write().await;
        registry.retire_by_name(skill_id);
        Ok(())
    }
}
```

### 16.4 生命周期状态流转

```text
+------------------------------------------------------------------+
|                  Skill 生命周期状态机                              |
|                                                                  |
|   [Draft] --(测试通过)--> [Ready] --(发布)--> [Published]        |
|      |                      |  ^                   |             |
|      |                      |  |                   |             |
|      |             (进化引擎精化)  (回滚)          |             |
|      |                      |  |                   |             |
|      |                      v  |                   |             |
|      |                  [Evolving]                  |             |
|      |                                             |             |
|      |          (30天未更新)   (30天未使用)   (手动废弃)           |
|      +----------------+------------+--------------+             |
|                        v            v                            |
|                   [Deprecated]                                   |
|                        |                                         |
|                 (90天+0次执行)                                   |
|                        v                                         |
|                 [提示用户删除]                                    |
+------------------------------------------------------------------+
```

---

## 17. 测试框架

### 17.1 SkillTestRunner

```rust
// crates/agent-runtime/src/skills/test_runner.rs

pub struct SkillTestRunner {
    pool: SqlitePool,
    mock_tools: HashMap<String, MockToolResponse>,
}

pub struct TestCase {
    pub name: String,
    pub params: serde_json::Value,
    pub expect_exit_code: Option<i32>,
    pub expect_output_contains: Option<String>,
    pub expect_output_not_contains: Option<String>,
    pub timeout_secs: u64,
}

pub struct TestResult {
    pub case_name: String,
    pub passed: bool,
    pub output: String,
    pub error: Option<String>,
    pub duration_ms: u64,
}

impl SkillTestRunner {
    /// 运行 Skill 的所有内置测试
    pub async fn run_all(&self, manifest: &SkillManifest) -> Vec<TestResult> {
        let mut results = Vec::new();

        for case_def in &manifest.test_cases {
            let case = TestCase {
                name: case_def.name.clone(),
                params: case_def.params.clone(),
                expect_exit_code: case_def.expect_exit_code,
                expect_output_contains: case_def.expect_output_contains.clone(),
                expect_output_not_contains: case_def.expect_output_not_contains.clone(),
                timeout_secs: manifest.timeout_secs.unwrap_or(60) as u64,
            };

            let result = self.run_one(manifest, &case).await;
            results.push(result);
        }

        results
    }

    async fn run_one(&self, manifest: &SkillManifest, case: &TestCase) -> TestResult {
        let start = std::time::Instant::now();

        // 1. 参数 Schema 校验
        if let Err(e) = validate_params(&manifest.parameters, &case.params) {
            return TestResult {
                case_name: case.name.clone(),
                passed: false,
                output: String::new(),
                error: Some(format!("参数校验失败: {}", e)),
                duration_ms: start.elapsed().as_millis() as u64,
            };
        }

        // 2. 在受控环境中执行
        let exec_result = tokio::time::timeout(
            Duration::from_secs(case.timeout_secs),
            self.execute_controlled(manifest, &case.params),
        ).await;

        match exec_result {
            Ok(Ok(output)) => {
                let passed = check_assertions(case, &output);
                TestResult {
                    case_name: case.name.clone(),
                    passed,
                    output,
                    error: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
            Ok(Err(e)) => TestResult {
                case_name: case.name.clone(),
                passed: false,
                output: String::new(),
                error: Some(e.to_string()),
                duration_ms: start.elapsed().as_millis() as u64,
            },
            Err(_) => TestResult {
                case_name: case.name.clone(),
                passed: false,
                output: String::new(),
                error: Some("执行超时".into()),
                duration_ms: case.timeout_secs * 1000,
            },
        }
    }
}
```

### 17.2 录制/回放

```rust
/// 录制模式：记录真实执行的工具调用序列
pub struct SkillRecorder {
    pub tool_calls: Vec<RecordedToolCall>,
}

pub struct RecordedToolCall {
    pub tool: String,
    pub params: serde_json::Value,
    pub result: serde_json::Value,
    pub duration_ms: u64,
}

impl SkillTestRunner {
    /// 注入录制数据，用回放替代真实工具调用
    pub fn with_recording(&mut self, recording: Vec<RecordedToolCall>) {
        for call in recording {
            self.mock_tools.insert(call.tool.clone(), MockToolResponse::Recorded(call.result));
        }
    }
}
```

录制文件存储在 Skill 目录的 `tests/` 子目录下：

```text
my-skill/
+-- tests/
    |- basic.recording.json       # 基本场景录制
    +- edge-case.recording.json   # 边界场景录制
```

### 17.3 与进化引擎的 Eval 集成

进化引擎在生成候选 Skill 后，使用 SkillTestRunner 做沙箱验证。验证通过率 >= 70% 才允许进入 A/B 测试阶段。详见 [05-自我进化引擎详细设计.md](../_v0.3规划/05-自我进化引擎详细设计.md) 第 3.2 节。

---

## 18. 市场发布

### 18.1 打包格式

发布到市场的 Skill 打包为 `.skill.zip`：

```text
deploy-vercel-1.2.0.skill.zip
|- deploy-vercel/             # Skill 目录（完整保留）
|   |- SKILL.md
|   |- scripts/
|   |   +- check-env.sh
|   |- templates/
|   |   +- report.md
|   |- examples/
|   |   +- example-usage.md
|   +- tests/
|       |- basic.recording.json
|       +- edge-case.recording.json
+- manifest.json              # 包元数据
```

`manifest.json` 包含：

```json
{
  "name": "deploy-vercel",
  "version": "1.2.0",
  "description": "一键将 Next.js 项目部署到 Vercel",
  "author": "astro-user",
  "license": "MIT",
  "checksum": "sha256:abc123...",
  "signature": "...",
  "min_astro_version": "0.5.0",
  "requires_tools": ["shell_exec", "file_read"],
  "tags": ["deploy", "vercel", "nextjs"]
}
```

### 18.2 发布流程

```text
用户执行 /publish-skill deploy-vercel
    |
    v
(1) 验证 status == ready 或 published
    |
    v
(2) 运行全部测试用例（必须全部通过）
    |
    v
(3) 检查向后兼容（若为版本更新）
    |
    v
(4) 打包为 .skill.zip（含签名）
    |
    v
(5) 上传到市场 API（见 02-Agent市场.md）
    |
    v
(6) 更新本地 status = published
```

### 18.3 安装流程

```text
用户从市场安装 deploy-vercel
    |
    v
(1) 下载 .skill.zip -> 验证签名和 checksum
    |
    v
(2) 解压到 {workspace}/.astro/skills/deploy-vercel/
    |
    v
(3) 自动运行 SkillTestRunner（失败则回滚安装）
    |
    v
(4) SkillRepo::insert() + SkillRegistry::update()
    |
    v
(5) 通知用户安装完成
```

详见 [02-Agent市场.md](../../03-系统设计阶段/09-生态扩展/02-Agent市场.md)。

---

## 19. Tauri Commands

```typescript
// == CRUD ==

// 列出工作区所有 Skill
invoke("list_skills", {
  workspaceId: string,
  includeGlobal?: boolean,    // 是否包含全局 Skill，默认 true
  statusFilter?: string[],    // 状态过滤，默认排除 deprecated
}) -> Promise<SkillInfo[]>

// 获取 Skill 详情（含完整正文）
invoke("get_skill", {
  skillName: string,
  workspaceId?: string,
}) -> Promise<SkillDetail>

// 创建 Skill（草稿）
invoke("create_skill", {
  workspaceId: string,
  name: string,
  content: string,            // 完整 SKILL.md 内容
}) -> Promise<SkillInfo>

// 保存/更新 Skill（自动 diff 生成 changelog）
invoke("save_skill", {
  skillName: string,
  content: string,
  changelog?: string,
}) -> Promise<SkillInfo>

// 删除 Skill（需先 deprecated）
invoke("delete_skill", {
  skillName: string,
}) -> Promise<void>

// == 调用 ==

// 手动调用 Skill
invoke("invoke_skill", {
  skillName: string,
  args?: string,              // 原始参数字符串
  conversationId: string,
}) -> Promise<SkillInvocationResult>

// == 版本管理 ==

// 查看版本历史
invoke("list_skill_versions", {
  skillName: string,
  workspaceId?: string,
}) -> Promise<SkillVersion[]>

// 回滚到历史版本
invoke("rollback_skill", {
  skillName: string,
  targetVersion: string,
}) -> Promise<void>

// == 测试 ==

// 运行 Skill 内置测试
invoke("run_skill_tests", {
  skillName: string,
}) -> Promise<TestResult[]>

// 录制一次执行作为测试用例
invoke("record_skill_execution", {
  skillName: string,
  caseName: string,
}) -> Promise<string>          // 返回录制文件路径

// == 生命周期 ==

// 标记为废弃
invoke("deprecate_skill", {
  skillName: string,
  reason: string,
  replacedBy?: string,
}) -> Promise<void>

// 导出 .skill.zip 包
invoke("export_skill_package", {
  skillName: string,
  outputDir: string,
}) -> Promise<string>          // 返回包路径

// 发布到市场
invoke("publish_skill", {
  skillName: string,
  releaseNotes: string,
}) -> Promise<MarketPublishResult>

// == Overrides ==

// 获取当前 Skill Overrides 配置
invoke("get_skill_overrides", {
  workspaceId?: string,
}) -> Promise<Record<string, string>>

// 设置 Skill Override
invoke("set_skill_override", {
  skillName: string,
  state: "on" | "name-only" | "user-invocable-only" | "off",
  workspaceId?: string,
}) -> Promise<void>
```

### TypeScript 类型

```typescript
// apps/desktop/src/types/skills.ts

interface SkillInfo {
  id: string;
  name: string;
  description: string;
  whenToUse?: string;
  version: string;
  status: "draft" | "ready" | "published" | "deprecated" | "evolving";
  origin: "builtin" | "synthesized" | "imported" | "marketplace";
  source: "builtin" | "global" | "workspace" | "subdirectory" | "plugin";
  riskLevel?: string;
  disableModelInvocation: boolean;
  userInvocable: boolean;
  overrideState?: "on" | "name-only" | "user-invocable-only" | "off";
  tags: string[];
  hasTests: boolean;
  createdAt: number;
  updatedAt: number;
}

interface SkillDetail extends SkillInfo {
  content: string;              // 完整 SKILL.md 内容
  processedContent?: string;    // 替换+注入后的正文（预览用）
  skillDir: string;
  entryFile: string;
  arguments: ArgumentDef[];
  argumentHint?: string;
  allowedTools?: string[];
  disallowedTools?: string[];
  hooks: Record<string, SkillHookDef>;
  testCases: TestCaseDef[];
  triggerPatterns: string[];
  triggerThreshold: number;
}

interface SkillVersion {
  id: string;
  version: string;
  changelog?: string;
  breaking: boolean;
  createdAt: number;
  createdBy: string;
}

interface TestResult {
  caseName: string;
  passed: boolean;
  output: string;
  error?: string;
  durationMs: number;
}

interface SkillInvocationResult {
  skillName: string;
  mode: "inline" | "subagent";   // 直接注入 vs Subagent 执行
  success: boolean;
  output?: string;
  subAgentResult?: SubAgentResult;
}

interface ArgumentDef {
  name: string;
  description?: string;
  required: boolean;
  default?: string;
}

interface SkillHookDef {
  command: string;
  onFailure?: "abort" | "continue" | "skip";
  timeoutMs?: number;
  toolFilter?: string;
}

interface TestCaseDef {
  name: string;
  params: Record<string, unknown>;
  expectExitCode?: number;
  expectOutputContains?: string;
  expectOutputNotContains?: string;
}
```

---

## 20. 前端组件

### 20.1 SkillList -- 技能列表

```typescript
// apps/desktop/src/components/skills/SkillList.tsx

export function SkillList({ workspaceId }: { workspaceId: string }) {
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [filter, setFilter] = useState<{
    status?: string[];
    origin?: string[];
    search?: string;
  }>({});

  useEffect(() => {
    invoke<SkillInfo[]>("list_skills", { workspaceId }).then(setSkills);
  }, [workspaceId]);

  const filtered = skills.filter(s => {
    if (filter.status && !filter.status.includes(s.status)) return false;
    if (filter.origin && !filter.origin.includes(s.origin)) return false;
    if (filter.search && !s.name.includes(filter.search)
        && !s.description.includes(filter.search)) return false;
    return true;
  });

  return (
    <div className="skill-list">
      <SkillListToolbar filter={filter} onFilterChange={setFilter} />
      {filtered.map(skill => (
        <SkillCard key={skill.id} skill={skill} />
      ))}
    </div>
  );
}
```

### 20.2 SkillEditor -- 技能编辑器

```typescript
// apps/desktop/src/components/skills/SkillEditor.tsx

export function SkillEditor({
  skillName,
  workspaceId,
}: {
  skillName?: string;
  workspaceId: string;
}) {
  const [detail, setDetail] = useState<SkillDetail | null>(null);
  const [content, setContent] = useState("");
  const [testResults, setTestResults] = useState<TestResult[]>([]);
  const [versions, setVersions] = useState<SkillVersion[]>([]);
  const [isDirty, setIsDirty] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const [preview, setPreview] = useState<string | null>(null);

  // 加载 Skill 详情
  useEffect(() => {
    if (skillName) {
      invoke<SkillDetail>("get_skill", { skillName, workspaceId }).then(d => {
        setDetail(d);
        setContent(d.content);
      });
      invoke<SkillVersion[]>("list_skill_versions", { skillName }).then(setVersions);
    }
  }, [skillName]);

  const runTests = async () => {
    if (!skillName) return;
    const results = await invoke<TestResult[]>("run_skill_tests", { skillName });
    setTestResults(results);
  };

  const save = async () => {
    if (!skillName) return;
    await invoke("save_skill", { skillName, content });
    setIsDirty(false);
  };

  return (
    <div className="skill-editor">
      {/* 顶部工具栏 */}
      <SkillEditorToolbar
        isDirty={isDirty}
        onSave={save}
        onRunTests={runTests}
        onViewHistory={() => setShowHistory(true)}
        onPublish={() => {/* publish flow */}}
        onDeprecate={() => {/* deprecate flow */}}
        onPreview={() => {/* preview processed content */}}
      />

      <div className="editor-body">
        {/* 主编辑区：YAML frontmatter + Markdown 正文 */}
        <div className="editor-main">
          <CodeEditor
            language="markdown"
            value={content}
            onChange={v => { setContent(v); setIsDirty(true); }}
          />
        </div>

        {/* 侧边面板 */}
        <div className="editor-sidebar">
          {/* frontmatter 可视化编辑 */}
          {detail && <FrontmatterPanel manifest={detail} />}

          {/* 测试结果面板 */}
          {testResults.length > 0 && (
            <TestResultPanel results={testResults} />
          )}

          {/* 版本历史 */}
          {showHistory && (
            <VersionHistorySidebar
              versions={versions}
              onRollback={version =>
                invoke("rollback_skill", { skillName, targetVersion: version })
              }
              onClose={() => setShowHistory(false)}
            />
          )}
        </div>
      </div>
    </div>
  );
}
```

### 20.3 SkillStore -- 技能市场

```typescript
// apps/desktop/src/components/skills/SkillStore.tsx

export function SkillStore({ workspaceId }: { workspaceId: string }) {
  const [catalog, setCatalog] = useState<MarketSkillEntry[]>([]);
  const [installing, setInstalling] = useState<Set<string>>(new Set());

  const install = async (entry: MarketSkillEntry) => {
    setInstalling(prev => new Set(prev).add(entry.name));
    try {
      await invoke("install_skill_from_market", {
        packageId: entry.id,
        workspaceId,
      });
    } finally {
      setInstalling(prev => {
        const next = new Set(prev);
        next.delete(entry.name);
        return next;
      });
    }
  };

  return (
    <div className="skill-store">
      <SearchBar onSearch={query => {/* search market API */}} />
      <div className="catalog-grid">
        {catalog.map(entry => (
          <MarketSkillCard
            key={entry.id}
            entry={entry}
            installing={installing.has(entry.name)}
            onInstall={() => install(entry)}
          />
        ))}
      </div>
    </div>
  );
}
```

---

## 21. 设计约束

### 21.1 资源限制

| 约束 | 默认值 | 可配置 | 说明 |
|------|--------|-------|------|
| 单个 Skill 正文最大 token 数 | 10,000 | 否 | 超出时系统提示截断，正文超长 Skill 应使用 `context: fork` |
| 所有 Skill 描述总预算 | 5,000 token | 否 | 常驻系统提示中的描述总量 |
| 上下文压缩后单个 Skill 保留 | 5,000 token | 否 | 压缩时每个 Skill 最多保留的 token 数 |
| 上下文压缩后 Skill 总预算 | 25,000 token | 否 | 压缩时所有 Skill 内容的总保留 token |
| 动态注入命令超时 | 5 秒 | 是 | 单条 `!command` 执行超时 |
| 动态注入命令输出上限 | 32 KB | 是 | 单条命令输出截断 |
| 版本历史保留数量 | 30 个版本 | 是 | 超出时删除最旧非 published 版本 |
| BM25 召回 top-k | 5 | 是 | 每次查询最多匹配 5 个 Skill |
| Skill Hook 优先级范围 | 450 | 否 | 固定在用户配置级（400-499）范围内 |

### 21.2 性能约束

| 操作 | 延迟目标 |
|------|---------|
| BM25 召回（100 个 Skill） | < 5ms |
| SKILL.md 解析（含 frontmatter） | < 2ms |
| 动态注入全流程 | < 10s（受命令执行时间影响） |
| 文件变更检测到索引更新 | < 1s |
| Skill 目录扫描（500 个 Skill） | < 500ms |

### 21.3 安全约束

| 约束 | 说明 |
|------|------|
| `allowed-tools` 不可升权 | Skill 只能授予 Agent 已有授权范围内的工具 |
| 动态注入命令受安全策略约束 | 命令黑名单 + 白名单 + 沙箱执行 |
| `context: fork` 遵循 Supervisor 安全模型 | 子 Agent 权限是父 Agent 权限的子集 |
| Skill Hooks 不可使用系统保留优先级 | 优先级固定 450，无法覆盖安全/隐私 Hook |
| Draft Skill 不参与 BM25 召回 | 未通过测试的 Skill 不会被自动触发 |
| Deprecated Skill 降权 | 废弃 Skill 的 BM25 得分乘以 0.1 |
| Override "off" 完全隐藏 | 被 off 的 Skill 不出现在任何系统提示中 |

### 21.4 工作区隔离

- 用户创建的 Skill 默认存放在当前工作区的 `.astro/skills/` 目录下
- 全局 Skill（`~/.astro/skills/`）对所有工作区可见
- 内置 Skill 对所有工作区可见且只读
- 从市场安装的 Skill 安装到指定工作区，不全局共享
- Skill Override 可在全局或工作区级别设置，工作区级覆盖全局级

---

## 22. 相关文档

| 文档 | 关联内容 |
|------|---------|
| [04-Skills系统.md](../../03-系统设计阶段/02-核心功能模块/04-Skills系统.md) | SKILL.md 基础格式、BM25 按需披露、调用机制 |
| [06-自我进化引擎.md](../../03-系统设计阶段/02-核心功能模块/06-自我进化引擎.md) | 进化触发条件、候选生成策略、evolution_log |
| [05-自我进化引擎详细设计.md](../_v0.3规划/05-自我进化引擎详细设计.md) | EvolutionEngine 完整实现、Create/Refine/Retire 策略、A/B 测试 |
| [06-子Agent派生详细设计.md](../01-核心引擎层/06-子Agent派生详细设计.md) | Supervisor、SpawnConfig、context:fork 执行路径 |
| [08-Hooks系统详细设计.md](../01-核心引擎层/08-Hooks系统详细设计.md) | HookRegistry、ShellHook、优先级分段 |
| [04-工作流编辑器.md](../../03-系统设计阶段/06-桌面端/04-工作流编辑器.md) | 工作流编辑器生成 Skill（创建方式 B） |
| [02-Agent市场.md](../../03-系统设计阶段/09-生态扩展/02-Agent市场.md) | `.skill.zip` 包格式、市场安装流程 |
| [04-多执行后端系统设计.md](../01-核心引擎层/04-多执行后端系统设计.md) | `exec_backend` 字段（local/docker） |
| [03-交互执行模式设计.md](../01-核心引擎层/03-交互执行模式设计.md) | `risk_level` 风险门控（L0-L3） |
