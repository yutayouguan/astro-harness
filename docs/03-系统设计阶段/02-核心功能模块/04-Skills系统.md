# Skills 系统

> **Harness 定位（2026-08-29）**：Skill 是按需加载的 scaffold/能力包；它可向 `PromptContract` 注入指令，也可通过 `astro_tools` additive 放宽 toolset，但不拥有 turn loop、审批或沙箱。这些始终由 Harness 执行。

> 文档状态：定稿 | 阶段：系统设计 | 拆分自：原 07-MCP与Skills与子Agent.md

> :warning: Skills 系统已在详细设计阶段进行了重大重新设计（对齐 Claude Code Skills 架构）。
> 本文档描述的是初始系统设计概念，最新完整设计见 [01-Skills系统详细设计](../../04-详细设计阶段/04-工具与扩展生态/01-Skills系统详细设计.md)。
> 主要变更：目录结构取代单文件、动态上下文注入(!`cmd`)、$ARGUMENTS 参数系统、多级发现、调用控制、Subagent 执行、Skill Hooks。

---

## 一、概念定义

Skill 是高于 Tool 的能力单元，一个 Skill 可以编排多个 Tool、调用 LLM、甚至派生子 Agent。

---

## 二、Skill Trait

```rust
#[async_trait]
pub trait Skill: Send + Sync {
    fn manifest(&self) -> &SkillManifest;
    async fn execute(
        &self,
        ctx: &mut AgentContext,
        input: SkillInput,
    ) -> anyhow::Result<SkillOutput>;
}

pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub can_spawn_agents: bool,         // 声明是否会派生子 agent
    pub allowed_tools: Vec<String>,     // 白名单，限制 skill 能用哪些工具
}
```

---

## 三、Skill 定义文件（SKILL.md 格式）

**唯一格式**：SKILL.md，YAML frontmatter + Markdown 说明体。TOML 格式已废弃。

```markdown
---
name: research
description: 对给定主题进行深度研究，返回结构化报告
version: 1.0.0
platforms: [desktop]
can_spawn_agents: true
allowed_tools: [http_request, file_read, browser_fetch]
input_schema:
  type: object
  properties:
    topic:    { type: string, description: "研究主题" }
    depth:    { type: string, enum: [quick, deep], default: deep }
    language: { type: string, default: zh-CN }
  required: [topic]
trigger_tags: [研究, 调研, 深度分析, research]
---

## 说明

本 Skill 会先用 `http_request` 搜索资料，再通过 `browser_fetch` 提取正文，
最后汇总为结构化 Markdown 报告。
```

### 目录结构

一个目录 = 一个 Skill：

```text
~/.astro/skills/
└── research/
    ├── SKILL.md        # 元数据 + 说明（必须）
    ├── scripts/        # 执行脚本（可选，支持多语言）
    │   ├── main.py     # Python 脚本
    │   ├── main.ts     # TypeScript 脚本
    │   ├── main.rhai   # Rhai 脚本（Rust 原生沙箱）
    │   └── main.wasm   # WASM 模块
    ├── prompts/        # Prompt 模板（可选）
    │   └── system.md
    └── references/     # 参考文档（可选，懒加载）
```

---

## 三-b、Skill 脚本多语言支持

> **设计原则**：Skill 脚本支持 Python、TypeScript、Rhai、WASM 四种运行时，覆盖主流开发者生态。脚本语言通过 SKILL.md 的 `runtime` 字段声明，系统自动选择对应的执行引擎。

### 支持的脚本运行时

| 运行时 | 文件 | 执行方式 | 沙箱隔离 | 适用场景 | 优先级 |
| ---- | ---- | ---- | ---- | ---- | ---- |
| **Python** | `main.py` | 子进程 (`python3`) | 进程级隔离 + 可选 Docker | 数据处理、AI/ML、爬虫、科学计算 | P1 |
| **TypeScript** | `main.ts` | Deno 子进程 (`deno run`) | Deno 权限沙箱 | Web API 调用、JSON 处理、前端开发者 | P1 |
| **Rhai** | `main.rhai` | 嵌入式 (rhai crate) | Rust 内置沙箱 | 轻量工具逻辑、配置计算、零依赖场景 | P0 |
| **WASM** | `main.wasm` | wasmtime | 内存隔离 + 能力模型 | 高安全要求、跨平台分发、第三方插件 | P2 |
| **无脚本** | — | 纯 Prompt Skill | — | 翻译、摘要、代码审查等纯 LLM 任务 | P0 |

### SKILL.md runtime 字段

```yaml
---
name: data-analyzer
description: 分析 CSV 数据并生成报告
runtime: python          # python | typescript | rhai | wasm | prompt（默认）
runtime_config:
  python:
    version: ">=3.10"
    dependencies:         # 首次运行自动安装到 Skill 虚拟环境
      - pandas>=2.0
      - matplotlib>=3.7
    timeout_secs: 60
  typescript:
    permissions:           # Deno 权限声明
      - --allow-net=api.example.com
      - --allow-read=./data
    timeout_secs: 30
---
```

### 脚本接口协议

所有语言的脚本遵循统一的 JSON stdin/stdout 协议：

```text
Astro Agent                          脚本进程
    │                                    │
    ├─── stdin: JSON 输入 ──────────────►│
    │    {                               │
    │      "input": { ... },             │  脚本执行逻辑
    │      "context": {                  │
    │        "workspace_id": "...",       │
    │        "conversation_id": "..."    │
    │      }                             │
    │    }                               │
    │                                    │
    │◄── stdout: JSON 输出 ──────────────┤
    │    {                               │
    │      "output": "结果文本或结构化数据",│
    │      "artifacts": [                │
    │        { "path": "report.pdf", ... }│
    │      ],                            │
    │      "error": null                 │
    │    }                               │
    │                                    │
    │◄── stderr: 日志（可选）─────────────┤
```

### Python 执行引擎

```rust
pub struct PythonSkillRunner {
    python_path: PathBuf,        // python3 二进制路径
    venv_dir: PathBuf,           // ~/.astro/skills/<name>/.venv/
}

impl PythonSkillRunner {
    /// 首次运行自动创建虚拟环境并安装依赖
    pub async fn ensure_venv(&self, skill: &SkillManifest) -> Result<()> {
        if !self.venv_dir.exists() {
            Command::new(&self.python_path)
                .args(["-m", "venv", self.venv_dir.to_str().unwrap()])
                .status().await?;
        }
        if let Some(deps) = &skill.runtime_config.python.dependencies {
            Command::new(self.venv_dir.join("bin/pip"))
                .args(["install"].into_iter().chain(deps.iter().map(|s| s.as_str())))
                .status().await?;
        }
        Ok(())
    }

    /// 执行 Python 脚本
    pub async fn execute(&self, skill: &SkillManifest, input: Value) -> Result<SkillOutput> {
        self.ensure_venv(skill).await?;
        
        let mut child = Command::new(self.venv_dir.join("bin/python"))
            .arg(skill.script_path("main.py"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        
        // stdin 写入 JSON
        child.stdin.take().unwrap()
            .write_all(serde_json::to_vec(&input)?.as_slice()).await?;
        
        // 超时控制
        let timeout = Duration::from_secs(
            skill.runtime_config.python.timeout_secs.unwrap_or(60) as u64
        );
        let output = tokio::time::timeout(timeout, child.wait_with_output()).await
            .map_err(|_| ToolError::Timeout { timeout_ms: timeout.as_millis() as u64 })??;
        
        // 解析 stdout JSON
        let result: ScriptOutput = serde_json::from_slice(&output.stdout)?;
        Ok(result.into())
    }
}
```

### TypeScript 执行引擎（Deno）

```rust
pub struct TypeScriptSkillRunner;

impl TypeScriptSkillRunner {
    pub async fn execute(&self, skill: &SkillManifest, input: Value) -> Result<SkillOutput> {
        let permissions: Vec<&str> = skill.runtime_config.typescript
            .permissions.iter().map(|s| s.as_str()).collect();
        
        let mut child = Command::new("deno")
            .arg("run")
            .args(&permissions)                    // Deno 沙箱权限
            .arg("--no-prompt")                    // 不弹交互式权限请求
            .arg(skill.script_path("main.ts"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        
        // 同 Python：stdin JSON → stdout JSON
        // ...
    }
}
```

> **为什么选 Deno 而非 Node.js**：Deno 内置权限沙箱（`--allow-net`/`--allow-read`/`--allow-write`），默认拒绝所有权限，Skill 必须在 SKILL.md 中显式声明所需权限。Node.js 没有内置沙箱，脚本可以访问文件系统和网络。

### 脚本运行时自动检测与降级

```rust
pub fn select_runner(skill: &SkillManifest) -> Result<Box<dyn SkillRunner>> {
    match skill.runtime.as_deref() {
        Some("python") => {
            if which::which("python3").is_ok() {
                Ok(Box::new(PythonSkillRunner::new()))
            } else {
                Err(SkillError::RuntimeNotFound("python3 未安装，请运行 brew install python3".into()))
            }
        }
        Some("typescript") => {
            if which::which("deno").is_ok() {
                Ok(Box::new(TypeScriptSkillRunner::new()))
            } else {
                Err(SkillError::RuntimeNotFound("deno 未安装，请运行 brew install deno".into()))
            }
        }
        Some("rhai") => Ok(Box::new(RhaiSkillRunner::new())),
        Some("wasm") => Ok(Box::new(WasmSkillRunner::new())),
        Some("prompt") | None => Ok(Box::new(PromptSkillRunner::new())),
        Some(other) => Err(SkillError::UnsupportedRuntime(other.to_string())),
    }
}
```

### 安全隔离策略

| 运行时 | 文件系统 | 网络 | 进程 | 超时 |
| ---- | ---- | ---- | ---- | ---- |
| Python | 限制在 Skill 目录 + workspace | 受 HumanGuard L2 审批 | 独立子进程 | 默认 60s |
| TypeScript (Deno) | `--allow-read` 显式声明 | `--allow-net` 显式声明 | 独立子进程 | 默认 30s |
| Rhai | 无文件系统访问 | 无网络访问 | 嵌入主进程 | 默认 10s |
| WASM | 沙箱内存隔离 | 能力模型白名单 | wasmtime 沙箱 | 默认 5s |

> **Docker 可选强化**：对于安全敏感场景，Python 和 TypeScript 脚本可配置在 Docker 容器内执行（复用 F-35 多执行后端的 `DockerBackend`），进一步隔离文件系统和网络。在 `workspace.yaml` 中配置 `skill_execution_backend: docker`。

---

## 四、Skill 按需披露（BM25 检索）

Agent 不在 system prompt 中静态列出所有 Skill，而是在每轮推理前通过 BM25（tantivy）动态检索与当前对话相关的 Skill，将 top-K 结果注入 system prompt，降低 token 消耗。

```text
触发流程：
用户输入 → 提取关键词 → BM25 检索 SkillRegistry
         → 子串匹配兜底 → 取 top-5 → 注入 system prompt 末尾
         → LLM 推理（知道哪些 Skill 可用）
```

### 关键参数

| 参数 | 默认值 | 说明 |
| ---- | ------ | ---- |
| `max_skills_injected` | 5 | 每轮最多注入 Skill 数 |
| `skill_description_max_tokens` | 80 | 单个 Skill 描述 token 上限 |
| `bm25_threshold` | 0.3 | 低于此分数不注入 |
| `index_rebuild_on_change` | true | 热加载后自动增量更新 BM25 索引 |

**向量检索方案**：桌面端本地使用 **sqlite-vec**（轻量，零外部依赖）。`qdrant-client` 保留作为企业/云部署可选方案，不在桌面端默认启用。

---

## 五、Skill 加载与热更新

```rust
// crates/agent-core/src/skills/loader.rs
// 支持热加载：监听 skills/ 目录变更，自动重新注册
pub async fn load_from_dir(dir: &Path) -> Result<SkillRegistry>;
```

---

## 相关文档

- [03-MCP集成.md](03-MCP集成.md) — MCP 集成设计
- [Subagent 系统设计](05-Subagent系统设计.md) — Codex V2 Agent Threads 设计
- `04-详细设计阶段/03-记忆与上下文/02-Skills系统详细设计.md` — Skills 详细设计
- `04-详细设计阶段/04-工具与扩展生态/05-Skill全生命周期设计.md` — Skill 生命周期
