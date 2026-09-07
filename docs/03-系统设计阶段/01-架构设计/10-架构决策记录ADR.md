# 架构决策记录（Architecture Decision Records）

> **现行决策（2026-08-29）**：Agent 按 `Model + Harness` 定义；Thread actor、SessionTask、StepContext、原生工具协议、append-only rollout 和 V2 Agent Threads 是当前基线。以下历史 ADR 可解释决策演进，但其 `Supervisor/delegate_task/Core EventBus/增量 Checkpoint/7-crate 拆分` 等结论已被 [Agent Harness 总体架构](11-Agent-Harness总体架构.md) 取代（当前为 24 个 crate）。

> 阶段：系统设计 | 状态：定稿 | 说明：记录所有关键架构决策的背景、方案、取舍与后果

本文档汇总 Astro Agent 项目在架构设计过程中的所有关键技术决策。每条 ADR 遵循统一格式：背景（为什么需要这个决策）、决策（选择了什么）、备选方案（考虑过的替代方案及其优劣）、结果（决策带来的正面和负面后果）。

决策一经采纳，后续变更需新增 ADR 并标注替代关系，旧 ADR 状态更新为"已替代"。

---

## 目录

| 编号 | 标题 | 状态 |
|------|------|------|
| ADR-001 | Tauri v2 而非 Electron | 已采纳 |
| ADR-002 | SQLite + SQLCipher 而非 PostgreSQL/IndexedDB | 已采纳 |
| ADR-003 | Rust Workspace 多 Crate 架构 | 已采纳 |
| ADR-004 | BM25 + 向量混合检索而非纯向量 | 已采纳 |
| ADR-005 | SKILL.md 文件驱动而非数据库驱动 Skills | 已采纳 |
| ADR-006 | 四层记忆架构 | 已采纳 |
| ADR-007 | HumanGuard 三级审批而非二元允许/拒绝 | 已采纳 |
| ADR-008 | WASM 插件沙箱而非 Native 插件 | 已采纳 |
| ADR-009 | Hooks 系统而非硬编码拦截器 | 已采纳 |
| ADR-010 | 本地优先 + 可选云同步 | 已采纳 |
| ADR-011 | Agent 执行采用 round_loop 而非 plan-then-execute | 已采纳 |
| ADR-012 | React + Zustand 前端而非 Svelte/Solid | 已采纳 |
| ADR-013 | 子 Agent 采用 Supervisor 模式而非 Actor 模型 | 已替代 |
| ADR-014 | Checkpoint 增量快照而非全量复制 | 已采纳 |
| ADR-015 | Provider 故障转移采用熔断器模式 | 已采纳 |

---

### ADR-001: Tauri v2 而非 Electron

**状态**：已采纳
**日期**：2026-06
**决策者**：架构组

#### 背景

Astro Agent 需要一个跨平台桌面框架来承载 AI Agent 的交互界面。桌面端是核心交付形态，对包体积、内存占用、安全隔离和 Rust 后端集成能力均有严格要求。Electron 虽是市场主流，但其 Chromium + Node.js 的技术栈带来包体积膨胀（150MB+）和内存高占用（200MB+）问题，与本地优先、轻量高效的产品定位存在根本矛盾。

#### 决策

选择 **Tauri v2** 作为桌面框架。Tauri v2 利用操作系统原生 WebView（macOS WebKit、Windows WebView2、Linux WebKitGTK）渲染前端，Rust 作为后端运行时，通过 IPC 权限控制实现安全隔离。

关键技术点：
- 前端通过 `invoke()` 命令和 `listen()` 事件与 Rust 后端通信
- `AppState` 以 `Arc` 引用注入到所有 Tauri Command，持有 agent-runtime 全部服务
- 流式响应通过 Tauri `Channel` 或 SSE-like event 推送到前端
- Tauri v2 的权限沙箱隔离 Web 上下文与底层 Rust 运行时

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| Electron | 生态最成熟、组件库丰富、社区活跃度高 | 包体积 150MB+、内存占用 200MB+、Node.js 全权限安全模型 |
| Qt（C++/Python） | 系统集成最佳、内存占用最低（~30MB）、原生控件 | UI 表现力受限于 QML、需 FFI 集成 Rust、学习曲线陡峭 |
| Flutter Desktop | Dart 生态增长快、Skia 渲染一致性好 | 与 Rust 集成需 FFI、桌面端仍为 beta 质量、缺乏 Web 技术生态 |

#### 结果

**正面**：
- 包体积约 10MB，为 Electron 的 1/15
- 内存占用约 50MB，用户体验显著优于 Electron
- Rust 后端与 Tauri Rust 层无缝集成，共享类型定义，避免跨语言序列化开销
- IPC 权限控制模型天然符合 Agent 安全隔离需求

**负面**：
- 生态相比 Electron 更年轻，部分插件（如系统托盘、深度链接）需自行适配或等待社区完善
- 不同操作系统 WebView 版本差异可能引入渲染不一致（macOS WebKit vs Windows WebView2 行为差异）
- 社区规模较小，遇到边缘问题时参考资源有限
- WebView 更新依赖操作系统，无法像 Electron 那样独立控制 Chromium 版本

**约束**：
- 前端代码需测试 Safari（WebKit）和 Chrome（WebView2）两个渲染引擎的兼容性
- 系统分享功能通过 `tauri-plugin-share` 实现，需适配 macOS Share Sheet 和 Windows 系统分享菜单

---

### ADR-002: SQLite + SQLCipher 而非 PostgreSQL/IndexedDB

**状态**：已采纳
**日期**：2026-06
**决策者**：架构组

#### 背景

Astro Agent 采用本地优先架构，所有用户数据（对话、记忆、知识库、Skill 元数据、审计日志）均需在客户端持久化存储。存储方案需满足：嵌入式部署（无独立进程）、静态加密（保护用户隐私）、支持全文检索（FTS5）和向量检索（sqlite-vec 扩展）、离线可用。

#### 决策

选择 **SQLite + SQLCipher + sqlite-vec** 作为统一存储方案。所有数据存储在单个加密数据库文件中，SQLCipher 提供 AES-256 透明加密，sqlite-vec 提供嵌入式向量检索，FTS5 提供全文检索。WAL 模式支持并发读写。密钥通过系统密钥链（`keyring` crate）管理，首次启动自动生成，不写入配置文件。

数据库访问层采用 Repository 模式，每张业务表对应专属 Repository struct（ConversationRepo、MessageRepo、MemoryRepo、SkillRepo、MediaTaskRepo、AuditLogRepo 等）。上层通过 Repository API 访问数据，不直接持有 sqlx 连接。连接池最大 5 条连接，每条连接在 `before_acquire` 回调中加载 sqlite-vec C 扩展。迁移文件通过 `sqlx::migrate!()` 编译期内嵌，自动按版本升序执行。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| PostgreSQL | 生产级成熟、pgvector 向量支持优秀、并发性能强 | 需独立进程，桌面端部署复杂度高，违背本地优先原则 |
| IndexedDB（浏览器原生） | 零部署成本、Tauri WebView 原生支持 | 无加密、无全文检索、无向量检索、存储量受限、无 SQL 能力 |
| qdrant 本地模式 | 向量检索性能优秀、生产级 | 需启动独立进程、与 SQLite 存储割裂、加密需单独方案 |
| LevelDB/RocksDB | 嵌入式、高性能 KV 存储 | 无 SQL 能力、无全文检索、需自行实现向量检索 |

#### 结果

**正面**：
- 单文件部署，零运维，符合桌面应用分发模型
- SQLCipher 一并加密向量索引、全文索引和业务数据，安全方案统一
- `sqlx` 编译期 SQL 校验，减少运行时查询错误
- sqlite-vec 与 FTS5 共享同一连接池，混合检索无跨进程通信开销
- 数据备份只需复制单个文件

**负面**：
- sqlite-vec 较新（2024），大规模向量检索性能待 POC 验证（10 万块级别），备选 qdrant 本地模式
- SQLite 并发写入受限（WAL 模式缓解但未根治），高频写场景需注意锁竞争
- SQLCipher 引入约 5-15% 性能开销
- cipher_page_size 配置不当可能影响大块读写性能

**约束**：
- SQLCipher 密钥必须在任何 SQL 执行前通过 `PRAGMA key` 设置
- 多工作区逻辑隔离通过 `workspace_id` 外键约束实现，共享同一 `agent.db` 文件
- 知识库向量索引和媒体文件按工作区物理独立存储于各自目录

---

### ADR-003: Rust Workspace 多 Crate 架构

> **已被后续演进细化**：当前 Workspace 已扩展至 24 个 crate（见 CLAUDE.md Crate Map），依赖方向和分层结构与本文 7 crate 描述不同。核心决策（Cargo Workspace + 依赖单向性）仍然有效。

**状态**：已采纳
**日期**：2026-06
**决策者**：架构组

#### 背景

Astro Agent 后端功能涵盖 Agent 核心循环、多 Provider 适配、WASM/Rhai 运行时、MCP 协议、评估套件等多个领域。单一 crate 将导致编译时间过长、依赖方向混乱、模块耦合度高。需要设计合理的 crate 拆分方案，确保依赖单向性和编译效率。

#### 决策

采用 **Cargo Workspace 多 crate** 架构，将系统拆分为 7 个 crate，依赖方向严格单向：

```text
agent-types          <- 零依赖的纯类型层（共享 trait + 枚举）
    ^
agent-providers      <- 实现 Provider traits（依赖 agent-types）
agent-core           <- 内置工具 + 存储层（依赖 agent-types）
    ^
agent-runtime        <- 执行循环 + HumanGuard（依赖 core + providers + types）
agent-mcp-server     <- MCP 暴露（依赖 runtime + types）
agent-evals          <- 评估套件（依赖 runtime + types）
```

关键设计：提取 `agent-types` 作为最底层纯类型 crate（零依赖，纯类型层），定义 `Tool`、`Skill`、`Message`、`Permission`、`RiskLevel`、`AgentEvent`、`HookEvent` 等跨 crate 共享的 trait 和枚举，打破 `agent-core` 与 `agent-providers` 之间的潜在循环依赖。`agent-core` 负责内置工具实现和存储层，`agent-providers` 负责多 Provider 适配，两者均只依赖 `agent-types` 而互不依赖。`agent-runtime` 是唯一同时依赖 `agent-core` 和 `agent-providers` 的 crate，通过 `EventEmitter` trait 与前端解耦。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| 单体 crate（monolithic） | 开发初期简单、无需管理 crate 间依赖 | 编译时间随代码增长线性增长、无法并行编译、模块边界模糊 |
| 更少 crate（3 个：core/runtime/tauri） | 管理成本低 | core 承担过多职责、Provider 和 Tool 耦合 |
| 更多 crate（每个 Provider 独立 crate） | 最大化并行编译、最小化依赖面 | 管理成本高、版本协调复杂 |

#### 结果

**正面**：
- 依赖倒置原则（DIP）通过 crate 边界强制落地，编译器拒绝循环依赖
- `sccache` + workspace 增量编译，单 crate 改动只重编译受影响 crate
- Provider 可独立替换/测试，不影响核心逻辑
- `agent-evals` 完全独立，评估不引入生产代码变更

**负面**：
- 跨 crate 类型引用需通过 `agent-types` 中转，增加间接层
- crate 间 API 变更需协调多个 `Cargo.toml`
- 新开发者需理解 7 个 crate 的职责边界

---

### ADR-004: BM25 + 向量混合检索而非纯向量

**状态**：已采纳
**日期**：2026-07
**决策者**：架构组

#### 背景

Astro Agent 的 Skill 召回和知识库 RAG 检索需要高召回率和高精度。纯向量检索在语义相似性上表现优秀，但对关键词精确匹配（如命令名 `deploy-vercel`、特定术语）容易遗漏。纯 BM25 在处理语义同义词时召回不足。两者各有盲区，需要互补方案。

#### 决策

采用 **tantivy BM25 倒排索引 + sqlite-vec KNN 向量检索 + RRF（Reciprocal Rank Fusion）融合** 的混合检索方案。

**Skill 检索**：使用 tantivy 多字段加权 BM25 检索，字段权重分配为 name x3、description x2、trigger_patterns x2、when_to_use x2、tags x1、content x1。Skill 正文按 BM25 得分与 `trigger_threshold`（默认 0.5）比较，超阈值的 Skill 正文注入系统提示。

**知识库 RAG 检索**：使用 FTS5 BM25 + sqlite-vec 余弦相似度 KNN 双路检索，结果通过 RRF 公式 `score = sum(1 / (k + rank_i))` 融合排序。知识库支持六种分块策略：段落分块、Token 分块、滑动窗口、语义分块、父子分块和命题分块，根据文档类型自动选择最优策略。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| 纯向量检索（sqlite-vec / qdrant） | 语义理解强、处理同义词好 | 关键词精确匹配弱、嵌入模型调用有延迟和成本 |
| 纯 BM25（FTS5 / tantivy） | 关键词精确匹配快速准确、零额外成本 | 无法理解语义同义词、对自然语言查询召回率低 |
| Elasticsearch | 成熟的混合检索方案、丰富的分析器 | 需独立 JVM 进程、内存占用高、桌面端部署不现实 |
| 纯 LLM 重排序 | 语义理解最强 | 每次检索需 LLM 调用、延迟高、成本高 |

#### 结果

**正面**：
- 混合检索同时覆盖精确关键词匹配和语义近似查询，召回率显著优于单一方案
- tantivy 纯 Rust 实现，嵌入式运行，无外部依赖
- RRF 融合算法简单稳定，无需训练
- Skill 检索的多字段加权 BM25 使 `trigger_patterns` 等元数据直接参与排序

**负面**：
- 需维护两套索引（BM25 倒排 + 向量索引），写入时双倍开销
- 向量化依赖 Embedding Provider，离线场景需本地 Embedding 模型（Ollama）
- RRF 的 k 参数需调优以平衡两路结果权重

---

### ADR-005: SKILL.md 文件驱动而非数据库驱动 Skills

**状态**：已采纳
**日期**：2026-07
**决策者**：架构组

#### 背景

Skills 系统是 Astro Agent 的可插拔能力单元。设计需要在以下需求间取得平衡：人类可读性（开发者直接编辑）、版本控制友好（Git 追踪变更）、与 Claude Code Skills 架构对齐（降低迁移成本）、支持热加载（文件变更即时生效）。

#### 决策

选择 **SKILL.md 文件驱动** 方案。每个 Skill 以一个目录为载体，`SKILL.md` 文件作为入口点，YAML frontmatter 声明元数据（name、description、when_to_use、调用控制、执行控制、参数定义、作用域限定、Hooks 等），Markdown 正文承载 Agent 执行时注入的指令上下文。SQLite 中的 `skills` 表仅作为索引缓存，文件系统是唯一真相源（Single Source of Truth）。文件变更通过 `notify` crate 实时监控，秒级内反映到 Agent 的可用 Skill 集合中。

Skill 目录可包含辅助资源：模板（`template.md` / `templates/`）、示例（`examples/`）、可执行脚本（`scripts/`）、测试录制（`tests/`）和静态资源（`assets/`）。正文支持动态上下文注入（`!`command`` 内联和 ` ```! ` 块语法，加载时执行 shell 命令替换为输出）和字符串替换（`$ARGUMENTS`、`$name`、`${ASTRO_*}` 环境变量）。

多级发现路径按优先级从高到低：子目录级 > 工作区级 > 全局级 > 插件级 > 内置级。同名 Skill 高优先级覆盖低优先级。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| 纯数据库驱动 | 查询高效、结构化强、支持复杂索引 | 不便于 Git 版本控制、编辑需通过 GUI 或 CLI、与 Claude Code 模型不对齐 |
| JSON 配置文件 | 结构化、机器可解析 | 人类可读性差、不适合承载长文本指令、注释支持弱 |
| YAML 配置文件 | 结构化且可读性较好 | 不适合混合结构化元数据与长文本正文 |
| 数据库 + 文件双源 | 灵活度最高 | 一致性维护复杂、容易出现数据漂移 |

#### 结果

**正面**：
- 开发者可直接用编辑器修改 SKILL.md，所见即所得
- Git 天然追踪 Skill 变更历史，支持 PR Review 工作流
- 与 Claude Code Skills 架构高度对齐，生态互通成本低
- 多级发现（内置 -> 全局 -> 工作区 -> 子目录）支持 monorepo 场景
- 热加载无需重启应用

**负面**：
- 文件系统扫描在 Skill 数量极多时（1000+）启动性能下降
- SQLite 索引与文件系统可能短暂不一致（最终一致性）
- YAML frontmatter 解析错误可能导致单个 Skill 加载失败

---

### ADR-006: 四层记忆架构

**状态**：已采纳
**日期**：2026-07
**决策者**：架构组

#### 背景

AI Agent 需要不同类型的记忆来支撑对话连续性、用户偏好学习和能力积累。不同记忆类型在生命周期、检索策略和存储方式上差异显著：短期对话上下文需要快速顺序访问，语义知识需要向量检索，用户画像需要持久冻结注入，能力索引需要 BM25 召回。统一存储方案无法兼顾这些差异。

原始设计为五层（含独立的用户建模层），但经过详细设计阶段审视，发现用户建模实质上是持久记忆的一个写入管道（LLM 推断引擎），而非独立存储层。

#### 决策

采用 **四层记忆架构**，每层对应不同的存储实现和注入方式：

| 层级 | 名称 | 实现 | 注入方式 |
|------|------|------|---------|
| L1 | 情节记忆 | SQLite 对话表 + FTS5 | `session_search` 工具按需检索 |
| L2 | 语义记忆 | embeddings 虚拟表 + FTS5 | `@` 引用按需检索 |
| L3 | 持久记忆 | MEMORY.md + USER.md 冻结快照 | 会话开始时注入系统提示 |
| L4 | 程序性记忆 | Skills BM25 索引（tantivy） | 渐进式披露（按需加载正文） |

用户建模（USER.md）集成在 L3 中，由 LLM 推断引擎在每次会话结束时自动更新，通过 `memory_manage` 工具的 `category='core_insight'` 条目写入。

各层交互关系：
- L1 情节记忆的短期对话在会话结束后由 `MemoryDistiller`（后台 tokio 任务）蒸馏提取到 L2/L3
- L2 语义记忆的写入前经过噪音过滤（规则引擎过滤约 40% 噪音）和安全扫描（Prompt 注入、凭证泄露、不可见 Unicode 检测）
- L3 持久记忆采用软替换策略（旧版本 UPDATE `valid_to` + 新版本 INSERT，同一事务），支持版本追溯
- L4 程序性记忆按"描述常驻、正文按需"策略注入——所有 Skill 的 description 常驻系统提示，正文仅在调用时加载
- `ForgetScheduler` 后台任务使用 Weibull 衰减函数对 L2/L3 记忆进行遗忘调度

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| 扁平记忆存储（单一 KV） | 实现简单、查询统一 | 无法针对不同记忆类型优化检索策略 |
| 图记忆（Knowledge Graph） | 关系推理能力强、结构化程度高 | 实现复杂度高、图查询性能不确定、嵌入式图数据库选型有限 |
| 五层架构（含独立用户建模层） | 概念清晰、职责最分离 | 用户建模本质是写入管道而非存储层，独立层增加认知负担 |

#### 结果

**正面**：
- 每层使用最适合的检索策略（FTS5 / 向量 KNN / 文件冻结注入 / BM25）
- 情节记忆（L1）实时性好，语义记忆（L2）语义召回准确
- 持久记忆（L3）以 Markdown 文件形式存在，用户可直接查看和编辑
- 程序性记忆（L4）与 Skills 系统统一，BM25 渐进式披露减少 System Prompt 膨胀
- Weibull 衰减遗忘调度器模拟人类记忆衰减曲线，自动清理低频记忆

**负面**：
- 四层交互增加系统复杂度，调试记忆注入行为需理解多层逻辑
- 记忆蒸馏（从 L1 提取到 L2/L3）依赖 LLM 质量，可能引入噪声
- 不同层之间可能存在信息重复

---

### ADR-007: HumanGuard 三级审批而非二元允许/拒绝

**状态**：已采纳
**日期**：2026-07
**决策者**：架构组

#### 背景

AI Agent 执行工具操作时存在不同级别的风险：读取文件几乎无风险，执行 Shell 命令中等风险，删除文件或网络请求高风险。简单的二元允许/拒绝模型无法平衡自主性和安全性——全部允许太危险，全部拒绝太低效。需要一个分级审批机制，让低风险操作自动放行、高风险操作强制人工确认。

#### 决策

实现 **HumanGuard 四级风险（Low/Medium/High/Critical）对应三级介入（L1 自动/L2 确认/L3 强制手动）** 的审批模型，并引入与之正交的 YOLO 开关和自适应规划：

| 风险等级 | 介入方式 | 超时行为 |
|---------|---------|---------|
| Low | L1 自动执行，无审批 | - |
| Medium | L2 弹窗确认，30s 超时自动通过 | 自动放行 |
| High | L3 强制手动确认，无超时 | 永久等待 |
| Critical | 强制暂停，即使 YOLO 模式也不可跳过 | 永久等待 |

三维控制体系（正交）：自适应规划控制任务复杂度、L1/L2/L3 控制工具风险、YOLO 开关控制信任程度。

HumanGuard 状态机包含四态：`Running`（正常运行）、`Pending`（等待审批）、`Paused`（L2 暂停）、`Takeover`（L3 完全接管）。白名单快速放行机制支持精确工具名和参数正则匹配。所有审批决定写入不可篡改的 `audit_logs` 表（只允许 INSERT，禁止 UPDATE/DELETE），确保完整的审计追溯链。

`ToolRegistry::execute_checked()` 是所有工具调用的唯一入口，在此强制校验调用方权限（静态权限检查）和 HumanGuard 动态审批。用户可通过 `ModifiedAndApproved` 审批结果修改工具参数后再执行。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| 二元允许/拒绝 | 实现简单、用户决策明确 | 低风险操作频繁打断用户，高风险操作无分级保护 |
| 全自动（无审批） | 执行效率最高、用户无感知 | 安全风险不可控，误操作无法拦截 |
| 基于规则的静态白名单 | 配置明确、可预测 | 灵活性差，新工具需手动配置白名单 |
| 全面暂停审批 | 安全性最高 | 用户体验极差，Agent 自主性丧失 |

#### 结果

**正面**：
- 低风险操作（文件读取、搜索）零中断自动执行，用户体验流畅
- 高风险操作（Shell 执行、文件删除）强制确认，安全性有保障
- YOLO 开关提供临时信任模式，适合开发者主动承担风险的场景
- 白名单机制允许按工作区定制放行规则
- 审计日志完整记录每次审批决定，不可篡改

**负面**：
- 风险等级分配需要精心设计，错误分级可能导致安全漏洞或过度打断
- L2 的 30s 超时自动通过策略在特定场景可能过于激进
- 三维正交控制增加用户理解成本

---

### ADR-008: WASM 插件沙箱而非 Native 插件

**状态**：已采纳
**日期**：2026-07
**决策者**：架构组

#### 背景

Astro Agent 需要一个插件机制来支持第三方扩展复杂 Skill 和自定义工具。插件运行在用户设备上，安全隔离至关重要——恶意或有缺陷的插件不应能访问文件系统、网络或进程空间。同时需要跨平台兼容和合理的性能。

#### 决策

选择 **wasmtime**（Bytecode Alliance 出品）作为 WASM 运行时，为复杂 Skill 和第三方插件提供沙箱执行环境。轻量动态工具使用 Rhai 脚本引擎（双轨策略：Rhai 用于轻量工具逻辑，WASM 用于复杂/高性能 Skill）。

插件通过 `plugin.toml` 声明 Capabilities（权限清单），包括网络访问（白名单域名）、存储（插件私有 KV）、工具调用（允许的内置工具列表）和事件订阅（允许订阅的事件列表）。安装时向用户展示完整权限清单并要求确认。宿主函数以 `astro_` 前缀规范（`astro_log`、`astro_emit`、`astro_subscribe`、`astro_tool_call` 等），提供日志、事件、存储、工具调用等受控接口。资源限制通过三重机制保障：Fuel 燃料机制限制 CPU（默认 10,000,000 fuel/调用，约 1 亿条 WASM 指令）、线性内存上限（默认 32 MiB）和单次调用超时（默认 5 秒）。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| V8 Isolates（deno_core） | JS/TS 生态庞大、用户学习成本低 | 运行时较重（~10MB+）、安全沙箱不如 WASM 严格 |
| Docker 容器 | 隔离性最强、完整系统环境 | 桌面端需安装 Docker、启动慢（秒级）、资源占用高 |
| Native 动态库（dylib/dll） | 性能最高、可用任意语言编写 | 无沙箱隔离、跨平台编译复杂、安全风险不可控 |
| Lua 脚本 | 嵌入式运行时极小（~200KB）、沙箱简单 | 沙箱能力有限、生态较小、复杂逻辑编写困难 |

#### 结果

**正面**：
- WASM 天然沙箱，内存隔离、无文件系统/网络直接访问，安全性最强
- wasmtime 由 Bytecode Alliance 维护，安全审计充分，Rust 原生 API
- 燃料机制精确控制 CPU 使用量，防止插件无限循环
- 用户可用 Rust/C/AssemblyScript 等任意编译到 WASM 的语言编写插件
- 与轻量 Rhai 脚本互补，覆盖从简单配置到复杂逻辑的全场景

**负面**：
- WASM 调试工具不如原生语言成熟，开发体验有待提升
- 宿主函数 ABI 设计需要精心规划，后续变更需保持兼容
- WASM 组件模型（Component Model）仍在演进中，长期稳定性需关注
- 用户学习成本高于 JS 插件方案

---

### ADR-009: Hooks 系统而非硬编码拦截器

> **已被后续实现细化**：当前采用 `HookRuntime` 下的 Plugin、Command/MCP、Gateway 和 legacy Shell 执行面，以及事件专属 typed request/outcome；不存在本文设想的统一 `HookRegistry/HookPipeline`、WASM Hook 优先级管线。现行契约见 [Hooks 系统详细设计](../../04-详细设计阶段/01-核心引擎层/08-Hooks系统详细设计.md)。

**状态**：已采纳
**日期**：2026-08
**决策者**：架构组

#### 背景

`AgentExecutor::round_loop()` 是 Agent 的执行主干。随着功能增长，隐私过滤（PrivacyMiddleware）、安全检查（SecurityPolicy）、可观测性埋点（SpanCollector）、预算管控（BudgetManager）、Prompt 注入防护（PromptGuard）等横切关注点以硬编码方式散落在主干代码中。每次新增拦截逻辑都需要修改核心代码，违反开闭原则，且拦截器之间的执行顺序不透明。

#### 决策

实现统一的 **Hook trait + HookRegistry + HookPipeline** 扩展机制。现有六类硬编码拦截器（PrivacyMiddleware、SecurityPolicy、SpanCollector、BudgetManager、PromptGuard、AuditStore）重构为内置 Hook 实现（PrivacyFilterHook、SecurityCheckHook、ObservabilityHook、BudgetCheckHook、PromptGuardHook、AuditLogHook），不再特殊处理。外部用户通过配置文件（`hooks.toml`）声明 Shell 命令 Hook（`ShellHook`），WASM 插件可注册 WASM Hook（`WasmPluginHook`）。所有 Hook 按优先级排序执行（数值越小越先），支持 fail-open 和 fail-closed 两种故障策略。

Hook 生命周期事件覆盖 18 种可观测/可拦截节点，按六大类划分：Agent 生命周期事件、轮次事件、LLM 调用事件、工具执行事件、消息处理事件和系统事件。

优先级分区：系统级（0-99）> 安全级（100-199）> 审计级（200-299）> 框架级（300-399）> 用户级（400-499）> 插件级（500+）。Skill 作用域 Hook 固定优先级 450，在用户全局 Hook（400-449）之后、插件 Hook 之前执行。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| 继续硬编码拦截器 | 实现简单、无抽象开销 | 每次新增拦截逻辑需修改核心代码、顺序不透明、不可配置 |
| 中间件链（Tower 风格） | 洋葱模型清晰、Rust 生态成熟 | 适合请求-响应模型，Agent 执行管线是多阶段事件模型，不完全匹配 |
| 事件总线（pub/sub） | 解耦彻底、扩展灵活 | 执行顺序不可控、难以实现"中止管线"语义、调试困难 |

#### 结果

**正面**：
- 核心代码稳定，新增拦截逻辑只需注册新 Hook 实现，无需修改 `round_loop`
- 18 种生命周期事件覆盖从 Agent 初始化到消息处理的完整管线
- 用户可通过配置文件声明 Shell 命令 Hook，无需编写 Rust 代码
- Skill 作用域 Hook 随 Skill 加载/卸载自动注册/注销，生命周期与 Skill 一致
- 优先级机制确保安全检查始终先于用户自定义逻辑执行

**负面**：
- Hook 管线引入少量运行时开销（动态分发、优先级排序）
- Hook 间交互（一个 Hook 修改数据影响后续 Hook）增加调试难度
- Hook 执行失败的 fail-open/fail-closed 语义需要用户正确理解

---

### ADR-010: 本地优先 + 可选云同步

**状态**：已采纳
**日期**：2026-06
**决策者**：架构组

#### 背景

AI Agent 存储大量用户隐私数据：对话内容、个人偏好、知识库文档、API 密钥等。用户对数据主权有强烈需求。同时，多设备使用场景（桌面 + 笔记本）和数据备份需求要求提供同步能力。需要在隐私保护和便利性之间取得平衡。

#### 决策

采用 **本地优先** 架构：所有数据默认存储在用户设备上的加密 SQLite 数据库中，应用完全离线可用。云同步作为可选功能提供，采用 **E2E 加密**（XChaCha20-Poly1305 + Argon2id 密钥派生），服务端零知识——只存储密文，无法解密任何用户数据。

密钥层次结构：用户密码经 Argon2id（内存硬化 KDF，默认 64 MiB）派生主密钥（MK），MK 通过 HKDF-SHA256 派生密钥加密密钥（KEK）和数据加密密钥（DEK）。DEK 按工作区隔离：全局 DEK 加密全局记忆/设置，每个工作区有独立 DEK 加密该工作区的对话/知识库/Skill。DEK 以 `Encrypt(KEK, DEK)` 形式存储在本地 SQLite 的 `crypto_keys` 表中。主密钥 MK 不持久化，每次使用时从密码重新派生。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| 云优先（SaaS 模式） | 多设备无缝同步、服务端 AI 能力强 | 隐私风险高、离线不可用、供应商锁定 |
| 纯本地（无同步） | 隐私保护最强、实现最简单 | 无多设备支持、无备份、设备丢失数据不可恢复 |
| 混合（部分云/部分本地） | 灵活度高 | 一致性维护复杂、用户难以理解数据在哪里 |

#### 结果

**正面**：
- 用户数据完全由用户控制，无第三方可读取
- 离线场景完全可用（搭配本地 Ollama 推理）
- E2E 加密确保即使服务端被入侵，用户数据仍安全
- 增量同步 + 冲突解决，支持多设备使用

**负面**：
- 首次使用需设置同步密码，用户体验稍有摩擦
- 密码丢失无法恢复云端数据（零知识设计的固有代价）
- 本地存储依赖设备可靠性，未启用同步时无备份保障
- E2E 加密同步实现复杂度高（密钥层次、增量协议、冲突解决）

---

### ADR-011: Agent 执行采用 round_loop 而非 plan-then-execute

**状态**：已采纳
**日期**：2026-07
**决策者**：架构组

#### 背景

Agent 执行模式有两种主流范式：先完整规划再批量执行（plan-then-execute），或每轮迭代式地调用 LLM、执行工具、注入结果再进入下一轮（round_loop / ReAct-like）。Astro Agent 需要支持流式响应、实时工具调用反馈和人工审批中断，对执行模式的灵活性有高要求。

#### 决策

采用 **`AgentExecutor::round_loop()` 迭代式执行**。每轮循环的完整流程：

1. 注入 Pending 消息队列中的追加消息
2. 调用 LLM（含自适应规划指令注入到 System Prompt）
3. 检测 LLM 输出是否含 `<agent_plan>` 块：若有则推送 `plan_confirmation_required` 事件，等待用户确认/修改/取消
4. 解析 tool_calls，通过 `ToolRegistry.execute_checked()` 逐一执行（经 HumanGuard 审批）
5. 将 tool_result 注入上下文
6. 写入 Checkpoint（每轮工具调用全部完成后）
7. 若 stop_reason 非 EndTurn 则继续下一轮

自适应规划作为 round_loop 的可选阶段嵌入，而非独立的 plan-then-execute 流程。Pending 消息队列（`PendingQueue`，纯内存实现）支持 Agent 运行中用户追加的消息在当前轮次完成后注入下一轮 context，不打断当前执行。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| Plan-then-execute（完整规划后执行） | 执行计划可预览、用户可提前修改 | 规划阶段无工具反馈、LLM 可能规划出无法执行的步骤、流式体验差 |
| 纯 ReAct 模式（思考-行动-观察） | 学术研究充分、框架成熟 | 思考过程暴露为文本消耗 token、无原生工具调用支持 |
| DAG 工作流引擎 | 可视化编排、并行执行、确定性强 | 灵活性差、无法处理开放式任务、需用户预先定义流程 |

#### 结果

**正面**：
- 每轮都有真实工具执行反馈，LLM 可根据结果动态调整后续策略
- 流式响应实时推送到前端，用户感知延迟低
- HumanGuard 审批自然嵌入每轮工具执行，中断和恢复无额外机制
- 自适应规划在复杂任务时自动触发，简单任务直接执行，两者统一于同一循环
- Pending 消息队列支持运行中追加指令，无需打断当前执行

**负面**：
- 每轮都需 LLM 调用，token 消耗高于一次规划批量执行
- LLM 可能陷入无效循环，需双重迭代预算硬限制（`TurnState.tool_rounds` 与 `IterationBudget`，默认均为 90）
- 调试复杂任务时需追踪多轮交互，日志量大

**约束**：
- 上下文窗口随轮次增长需搭配上下文压缩策略（`maintain_tool_context()` 三阶段：prune 截断超大 tool 结果 → LLM 辅模型摘要 `AuxiliaryTask::Compaction` → head/tail fallback）；`compressed_content` 字段存 Provider 视图，`content` 字段永远保留原文
- 迭代预算管理采用双重机制并行生效：`IterationBudget`（每 Agent Thread 独立预算，`code_exec` 类型轮次可 refund）与 `TurnState`（每条用户消息归零的轮次计数，上限 `config.multi_turn`）

---

### ADR-012: React + Zustand 前端而非 Svelte/Solid

**状态**：已采纳
**日期**：2026-06
**决策者**：架构组

#### 背景

前端框架选型需考虑：与 Tauri v2 的集成成熟度、组件库生态（特别是 shadcn/ui 支持）、流式渲染支持（Agent 流式输出的实时展示）、团队招聘难度和学习曲线。

#### 决策

选择 **React + TypeScript** 作为前端框架，**Zustand** 作为状态管理方案。完整前端技术栈：

| 用途 | 选型 |
|------|------|
| UI 组件 | shadcn/ui + Radix UI |
| 样式 | Tailwind CSS v4 |
| 路由 | TanStack Router |
| 构建 | Vite |
| 代码高亮 | Shiki |
| 音频可视化 | wavesurfer.js |
| Markdown 渲染 | react-markdown + remark-gfm |
| 动画 | Motion（Framer Motion v12） |

前端状态通过 Zustand 统一管理，保持单向数据流：`dispatch action -> Tauri invoke -> Rust event -> Zustand store update`。禁止跨 store 直接修改。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| Vue 3 + Pinia | 学习曲线平缓、模板语法直观、有 shadcn-vue | Tauri 示例较少、组件库生态不及 React |
| Svelte | 运行时性能最佳、编译时框架、包体积极小 | 无 shadcn/ui 支持、Tauri 模板极少、社区规模小 |
| SolidJS | 细粒度响应式、运行性能极佳 | 无 shadcn/ui 支持、Tauri 示例几乎没有、招聘极难 |

状态管理备选：

| 方案 | 优势 | 劣势 |
|------|------|------|
| Redux Toolkit | 最成熟、DevTools 强大、适合大型应用 | 样板代码多、学习曲线陡 |
| Jotai | 原子化状态、API 简洁 | 适合细粒度状态，桌面应用整体状态管理模式不太匹配 |

#### 结果

**正面**：
- React 生态最成熟，Tauri 官方示例和模板最丰富
- shadcn/ui 提供高质量、可定制的 UI 组件，加速界面开发
- Zustand 样板代码极少，与 Tauri IPC 结合简洁自然
- TypeScript 一流支持，编译期类型检查减少运行时错误
- 招聘市场 React 开发者最多

**负面**：
- React 运行性能不如 Svelte/Solid，但对桌面应用场景影响不大
- Zustand 在极大型应用中可能不如 Redux 的结构化约束
- React 的虚拟 DOM 开销在长对话列表渲染时需要虚拟化优化

**约束**：
- 流式渲染（Agent 流式输出）通过 react-markdown 实时渲染，需处理不完整 Markdown 片段的容错
- 多模态媒体展示（音频波形、代码高亮）需额外库支撑（wavesurfer.js、Shiki）

---

### ADR-013: 子 Agent 采用 Supervisor 模式而非 Actor 模型

> **已被后续实现取代**：当前子 Agent 系统采用 V2 Agent Threads 模型（`agent-subagents`），使用 6 个模型工具（`spawn_agent`、`list_agents`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`），资源配额为 `max_threads=32`、`max_depth=8`、`max_running=8`。不再使用本文描述的 `Supervisor`、`delegate_task` 和 `SubAgentResult` 结构。现行契约见 [Agent Harness 总体架构](11-Agent-Harness总体架构.md) §11。

**状态**：已替代
**日期**：2026-07
**决策者**：架构组

#### 背景

Astro Agent 支持子 Agent 派生以处理复杂子任务。子 Agent 是完全隔离的 `AgentCore` 实例，对父 Agent 的对话历史一无所知。需要一个机制来管理子 Agent 的生命周期：派生、监控、取消、资源限制和结果收集。

#### 决策

采用 **Supervisor 模式**：`Supervisor` 结构体持有所有子 Agent 的句柄（`AgentHandle`），负责生命周期管理。父 Agent 通过 `delegate_task` 工具派生子 Agent（支持单任务和批量并行两种形式），Supervisor 强制执行深度限制（`max_depth` 1-3，默认 1）、并发限制（`max_concurrent_children` 默认 3）和硬超时（`timeout_secs` 默认 300s）。

子 Agent 设计哲学：**完全隔离的 AgentCore 实例**，对父 Agent 的对话历史一无所知。父 Agent 通过 `context` 字段主动传入所有所需信息；子 Agent 只把最终结构化摘要（`SubAgentResult`：actions、findings、modified_files、issues、token_usage、timed_out）回传给父 Agent，以此保持 Token 高效。

父 Agent 级联取消通过 `CancellationToken` 传播。硬超时通过 `tokio::time::timeout` 包装子 Agent 的 `round_loop`，超时后先发送 CancellationToken，等待 5s 优雅关闭，再强制 abort。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| Actor 模型（Actix） | 解耦彻底、消息传递模式成熟、并发性强 | 过度工程化、Actor 间通信复杂度高、对 Agent 场景"一次性子任务"不匹配 |
| 消息传递（channels only） | 最低抽象、直接使用 tokio channels | 无生命周期管理、无深度限制、资源泄漏风险 |
| 进程隔离（子进程） | 隔离性最强、崩溃不影响父进程 | 进程间通信开销大、序列化成本高、共享资源（LLM 连接池）困难 |

#### 结果

**正面**：
- Supervisor 提供统一的生命周期管理：派生、监控、取消、资源限制
- 深度限制（最大 3 层）防止递归失控，`max_depth=3` 时最多 27 个叶子节点
- 子 Agent 共享 `Arc<dyn LlmClient>` 连接池，避免重复创建 HTTP 连接
- 硬超时 + CancellationToken 双保险防止子 Agent 无限运行
- 7 类强制屏蔽工具（`delegate_task`、`git_push`、`memory_write` 等）防止副作用扩散
- 结构化摘要（`SubAgentResult`）控制信息回传粒度，避免 token 浪费

**负面**：
- Supervisor 是中心化管理，大量并发子 Agent 时可能成为瓶颈
- 子 Agent 完全隔离意味着无法共享父 Agent 的对话上下文，需父 Agent 显式传递
- 清理机制依赖 tokio 的 task abort，极端情况下可能有资源泄漏

---

### ADR-014: Checkpoint 增量快照而非全量复制

**状态**：已采纳
**日期**：2026-08
**决策者**：架构组

#### 背景

Astro Agent 需要 Checkpoint 机制来支持对话回滚、状态复现、分支探索和崩溃恢复。在长对话中，消息历史可能达到数百条甚至上千条。每次 Checkpoint 如果全量复制所有消息、记忆和上下文数据，存储开销将快速增长。需要一个既能精确恢复状态、又能控制存储成本的快照方案。

#### 决策

采用 **增量 Checkpoint** 方案。`messages` 表作为追加写的权威日志，Checkpoint 只记录元数据（轮次编号、模型、token 使用量、活跃子 Agent 列表、最后 tool_result ID 等），不复制消息本身。恢复时读取完整消息历史，通过 `repair_dangling_tool_calls()` 修复崩溃时未完成的 tool_call——收集所有已有 tool result 的 ID，找出最后一条 assistant 消息中缺少对应 tool result 的 tool_call_id，仅对这些注入占位 result（"进程意外重启，操作未完成，请重新规划"），已完成的工具不会重复执行。

Checkpoint 类型分为四种：`auto`（每轮自动）、`manual`（用户手动创建，可附加标签和描述）、`workflow`（DAG 节点边界自动创建）、`pre_risk`（高风险工具执行前自动创建）。自动 Checkpoint 在每轮工具调用全部完成后覆盖写入。

Checkpoint 与对话分支的关系：fork-on-restore 模式在从 Checkpoint 创建分支时注入完整的记忆和上下文快照，确保分支后的 Agent 状态与原始时刻完全一致。与崩溃恢复机制互补：崩溃恢复是"尽力而为"的最后可恢复状态，Checkpoint 是主动的精确快照。

启动时恢复流程按 `depth ASC` 排序：先恢复根任务，子任务由根任务编排逻辑重新 spawn。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| 全量复制（每次 Checkpoint 复制所有消息） | 恢复逻辑简单、状态完全自包含 | 存储开销线性增长、长对话 Checkpoint 耗时长 |
| 差分快照（只存储与上次的差异） | 存储效率高于全量 | 恢复需链式回放所有差分、差分链断裂时不可恢复 |
| 仅依赖消息日志（无显式 Checkpoint） | 零额外存储开销 | 无法精确记录非消息状态（如记忆快照、SystemPrompt 变化） |

#### 结果

**正面**：
- Checkpoint 元数据极小（JSON 序列化几百字节），频繁写入无压力
- `messages` 表的追加写特性天然保证已完成工具调用的持久性
- `repair_dangling_tool_calls()` 精确修复崩溃点，不重复执行已完成工具
- 支持 fork-on-restore：从 Checkpoint 创建分支时注入完整记忆快照
- 四种 Checkpoint 类型覆盖自动、手动、工作流和风险保护场景

**负面**：
- 增量方案依赖 `messages` 表的完整性，若消息丢失则 Checkpoint 失效
- 非消息状态（记忆变更、SystemPrompt 变化）的精确恢复需额外数据记录
- 崩溃恢复是"尽力而为"——LLM 本质非确定性，恢复后行为可能与崩溃前不同

---

### ADR-015: Provider 故障转移采用熔断器模式

> **已被后续实现细化**：Agent primary/fallback 只接受 Responses-capable targets，并且只允许首个可见 chunk 前切换；Chat、Anthropic、Gemini 与 Interactions adapter 仅服务非 Agent 调用。现行契约见 [Responses 原生 Agent 运行时架构](12-Responses原生Agent运行时架构.md)。

**状态**：已采纳
**日期**：2026-07
**决策者**：架构组

#### 背景

Astro Agent 对接多个 AI Provider（Anthropic、OpenAI、Google、DeepSeek、MiniMax、Ollama）。外部 API 可能因限流、服务故障或网络问题而暂时不可用。简单的重试策略在 Provider 持续故障时会造成请求堆积和延迟放大。需要一个自适应的故障转移机制，在 Provider 故障时快速切换到备选 Provider，在故障恢复后自动回切。

#### 决策

实现 **`FailoverClient` + `CircuitBreaker` 熔断器** 模式。`FailoverClient` 位于 `agent-providers/src/failover.rs`，持有按优先级排序的 Provider 列表和每个 Provider 对应的 `CircuitBreaker` 实例。`CircuitBreaker` 状态机包含三态：

```text
Closed（正常）─── 连续失败达阈值 ──→ Open（熔断）
    ↑                                    │
    │                                    │ 超时后
    └─── 探测成功 ←── HalfOpen（探测） ←─┘
```

`ProviderError` 统一枚举定义于 `agent-types`，包含 11 个变体，每个变体实现 `is_retryable()` 和 `should_failover()` 方法，指示重试和故障转移决策。重试采用指数退避（exponential backoff），通过 `agent-runtime/src/retry.rs` 中的 `with_retry()` 函数实现。熔断后请求自动路由到优先级次高的 Provider。`NetworkMonitor`（`agent-runtime/src/provider/network_monitor.rs`）持续检测网络连通性，离线时直接路由到本地 Ollama（通过 `OllamaClient` 实现 `TextClient` + `EmbeddingClient`，支持离线推理和嵌入）。

支持的 Provider 及其适配器架构：`AnthropicClient`（Messages API）、`OpenAIChatClient`（Chat Completions）、`OpenAIResponsesClient`（Responses API）、`OpenAICompatClient`（纯兼容，适配月之暗面等厂商）、`DeepSeekClient`（专属处理 `reasoning_content` 回传）、`MiniMaxClient`（专属处理 `reasoning_split` / `base_resp`）、`GoogleInteractionsClient`（Interactions API，默认用于对话）、`GoogleGenerateContentClient`（generateContent API，用于批量嵌入/RAG）、`OllamaClient`（本地模型）。

#### 备选方案

| 方案 | 优势 | 劣势 |
|------|------|------|
| 简单重试（固定次数） | 实现简单 | Provider 持续故障时重试浪费时间、无故障转移能力 |
| 负载均衡（轮询） | 请求均匀分布 | 不感知 Provider 健康状态、故障 Provider 仍接收请求 |
| 手动切换 | 用户完全控制 | 响应慢、依赖用户干预、自动化程度低 |
| 健康检查 + 主动探测 | 提前发现故障 | 额外网络开销、探测请求消耗 API 配额 |

#### 结果

**正面**：
- 熔断器快速隔离故障 Provider，避免请求堆积和级联失败
- HalfOpen 探测自动检测恢复，无需人工干预即可回切
- `ProviderError` 分类（retryable / failover / fatal）使重试和故障转移决策精确
- 多 Provider 优先级列表确保始终有备选方案
- 与 `NetworkMonitor` 集成，离线时自动降级到本地 Ollama

**负面**：
- 熔断器参数（失败阈值、超时时间、探测间隔）需要根据各 Provider 特性调优
- 不同 Provider 的模型能力差异可能导致故障转移后输出质量下降
- 多 Provider 切换增加 API 密钥管理复杂度

---

---

## ADR 间的关联关系

```text
ADR-001 Tauri v2
    ├── ADR-012 React + Zustand（Tauri 前端框架选型）
    └── ADR-003 Rust Workspace（Tauri Rust 后端的 crate 拆分）

ADR-002 SQLite + SQLCipher
    ├── ADR-004 BM25 + 向量混合检索（FTS5 + sqlite-vec 均在 SQLite 上）
    ├── ADR-010 本地优先（SQLite 是本地存储核心）
    └── ADR-014 Checkpoint 增量快照（messages 表作为权威日志）

ADR-003 Rust Workspace
    ├── ADR-008 WASM 插件沙箱（wasmtime 在 agent-runtime crate）
    └── ADR-009 Hooks 系统（Hook trait 在 agent-core，集成在 agent-runtime）

ADR-005 SKILL.md 文件驱动
    ├── ADR-004 BM25 + 向量混合检索（Skill BM25 召回）
    └── ADR-006 四层记忆架构（L4 程序性记忆 = Skills 索引）

ADR-007 HumanGuard 三级审批
    └── ADR-011 round_loop（HumanGuard 自然嵌入每轮工具执行）

ADR-013 Supervisor 模式
    └── ADR-011 round_loop（子 Agent 执行独立 round_loop）
```

---

## 附录：ADR 变更日志

| 日期 | ADR | 变更说明 |
|------|-----|---------|
| 2026-06 | ADR-001 ~ ADR-003, ADR-010, ADR-012 | 初始创建，项目技术栈选型 |
| 2026-07 | ADR-004 ~ ADR-008, ADR-011, ADR-013, ADR-015 | 核心架构决策 |
| 2026-08 | ADR-009, ADR-014 | 详细设计阶段补充决策 |
| 2026-08 | ADR-006 | 五层记忆架构简化为四层 |

---

## 术语表

| 术语 | 含义 |
|------|------|
| round_loop | Agent 执行主循环，每轮包含 LLM 调用、工具执行和结果注入 |
| HumanGuard | 人工审批守卫，按风险等级拦截工具调用并决定是否需要人工确认 |
| RRF | Reciprocal Rank Fusion，两路检索结果的融合排序算法 |
| Fuel | wasmtime 的 CPU 计量单位，用于限制 WASM 插件执行时间 |
| DEK | Data Encryption Key，数据加密密钥，按工作区隔离 |
| DIP | Dependency Inversion Principle，依赖倒置原则 |
| WAL | Write-Ahead Logging，SQLite 的预写日志模式，支持并发读写 |
| YOLO | 临时跳过 L2/L3 审批的信任开关，费用超限时强制关闭 |
