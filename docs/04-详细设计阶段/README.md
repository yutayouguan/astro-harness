# 详细设计阶段文档索引

> 阶段：详细设计 | 说明：本目录是系统设计的深化，包含具体算法、数据结构、代码级实现方案

---

## 01-核心引擎层

| 文件 | 说明 |
| ---- | ---- |
| [01-agent-core详细设计.md](01-核心引擎层/01-agent-core详细设计.md) | `Session` 为中心的 Harness 编排：runtime/tasks/streaming/prompt/control/timeline |
| [02-agent-runtime详细设计.md](01-核心引擎层/02-agent-runtime详细设计.md) | `AstroThread -> submission_loop -> SessionTask -> multi_turn` 当前执行链与旧 runtime 迁移边界 |
| [03-交互执行模式设计.md](01-核心引擎层/03-交互执行模式设计.md) | F-39/M-38 交互执行模式：自适应规划、Pending 消息队列、YOLO 开关、MultiTask |
| [04-多执行后端系统设计.md](01-核心引擎层/04-多执行后端系统设计.md) | 本地子进程、跨平台 sandbox、managed network、Browser/MCP 运行环境与规划边界 |
| [05-成本预算控制详细设计.md](01-核心引擎层/05-成本预算控制详细设计.md) | BudgetManager：三级预算模型、Token 计费、check_before_call 拦截、渐进降级、YOLO 强制关闭、费用统计报表 |
| [06-Subagent详细设计.md](01-核心引擎层/06-Subagent详细设计.md) | Subagent / Codex V2 Agent Threads：六工具分发、运行时派生与恢复、持久 Agent Graph/mailbox/status、真实 Session 时间线 |
| [07-Agent生命周期详细设计.md](01-核心引擎层/07-Agent生命周期详细设计.md) | Thread/Session/Task/Turn/Step/Attempt 六级生命周期，以及完整 Realtime、Elicitation、TurnSettings、Guardian retry 和用户 Shell 控制边界 |
| [08-Hooks系统详细设计.md](01-核心引擎层/08-Hooks系统详细设计.md) | Plugin、Command/MCP、Gateway、Shell 的 typed lifecycle、信任、异步运行与事件专属结果契约 |
| [09-Checkpoint与状态快照详细设计.md](01-核心引擎层/09-Checkpoint与状态快照详细设计.md) | rollout append-only 事实源、snapshot + live boundary 与 SessionStore 可重建投影 |
| [10-自主决策与行为进化详细设计.md](01-核心引擎层/10-自主决策与行为进化详细设计.md) | 自主进化：BehaviorEngine 行为策略、反馈信号收集、偏好学习模型、Prompt 自优化、工具模式学习、主动决策引擎 |
| [11-Agent协作协议详细设计.md](01-核心引擎层/11-Agent协作协议详细设计.md) | V2 Agent Graph/mailbox 已实现基础和 Debate/MapReduce/Voting 等目标协作协议的明确分界 |
| [12-Agent事件与恢复详细设计.md](01-核心引擎层/12-Agent事件与恢复详细设计.md) | `Op/EventMsg/TurnItem` 统一协议、rollout 事件源、live boundary 与 SessionStore 投影 |
| [13-Astro统一配置系统详细设计.md](01-核心引擎层/13-Astro统一配置系统详细设计.md) | Agent/项目分层配置、信任边界、热加载和运行时快照 |
| [14-Agent-Harness执行外壳详细设计.md](01-核心引擎层/14-Agent-Harness执行外壳详细设计.md) | Thread/Session/Task/Turn/Step/Attempt 执行层级，Reason→Act→Observe 闭环及安全、恢复和测试契约 |
| [15-Agent-Tree状态投影详细设计.md](01-核心引擎层/15-Agent-Tree状态投影详细设计.md) | 根会话 Agent Tree 的持久图、活动游标、根服务层级、Desktop 快照与增量投影契约 |

## 02-Provider与模型层

| 文件 | 说明 |
| ---- | ---- |
| [01-agent-providers详细设计.md](02-Provider与模型层/01-agent-providers详细设计.md) | Agent Responses-only 路由、原生 `ResponseItem`、通用 Provider/媒体兼容边界与 fallback |
| [02-多模型对比系统设计.md](02-Provider与模型层/02-多模型对比系统设计.md) | F-33/US-082 多模型对比：tokio 并发流式调用、CompareView 并排 UI、选优采纳、费用独立统计 |
| [03-Provider故障转移设计.md](02-Provider与模型层/03-Provider故障转移设计.md) | F-14 故障转移：错误可重试性分类、指数退避+Jitter、熔断器、Provider 自动切换、流式续传、全链路降级 |
| [04-离线与本地模型详细设计.md](02-Provider与模型层/04-离线与本地模型详细设计.md) | F-18 离线能力：Ollama Sidecar 生命周期、本地模型发现与注册、NetworkMonitor 离线检测、自动降级切换、在线恢复 |
| [05-模型洞察界面详细设计.md](02-Provider与模型层/05-模型洞察界面详细设计.md) | F-26 模型洞察：使用统计、性能排行榜、成本分析与投影、模型推荐引擎、数据聚合优化 |

## 03-记忆与上下文

| 文件 | 说明 |
| ---- | ---- |
| [01-记忆系统详细设计.md](03-记忆与上下文/01-记忆系统详细设计.md) | 5 层记忆架构：情节/语义/持久/程序性/用户建模，KNN 向量检索、Weibull 衰减遗忘、MEMORY.md 快照、BM25 技能召回 |
| [02-上下文压缩算法设计.md](03-记忆与上下文/02-上下文压缩算法设计.md) | tool context prune、LLM compaction、head/tail fallback、spill 和原始/压缩视图分离 |
| [03-Persona与SystemPrompt注入设计.md](03-记忆与上下文/03-Persona与SystemPrompt注入设计.md) | PromptContract：稳定基础指令、角色化 dynamic context、用户输入与独立工具 schema |
| [04-知识库RAG详细设计.md](03-记忆与上下文/04-知识库RAG详细设计.md) | F-05 知识库RAG：文档导入管道、RecursiveChunker 分块算法、BM25+向量混合检索（RRF）、knowledge_manage 工具 |
| [05-记忆图谱详细设计.md](03-记忆与上下文/05-记忆图谱详细设计.md) | 记忆图谱：实体/关系抽取、SQLite 图查询（递归 CTE）、图谱增强检索、实体去重、vis-network 可视化 |
| [07-线程检查点与上下文续接.md](03-记忆与上下文/07-线程检查点与上下文续接.md) | notes CAS、canonical history 引用、采样占用、turn-scoped 压缩及恢复验收 |
| [_历史参考/用户建模系统设计.md](03-记忆与上下文/_历史参考/用户建模系统设计.md) | ⚠️ 已废弃 — 原用户建模设计，已被 01-记忆系统详细设计 的 L5 层取代，仅保留历史参考 |

## 04-工具与扩展生态

| 文件 | 说明 |
| ---- | ---- |
| [01-Skills系统详细设计.md](04-工具与扩展生态/01-Skills系统详细设计.md) | Skills 系统：发现/安装、`ExtensionSnapshot` 索引、`skills` 原生工具、canonical output、`astro_tools` additive gate 与恢复边界 |
| [02-工具系统详细设计.md](04-工具与扩展生态/02-工具系统详细设计.md) | `CoreToolRuntime -> ToolRegistry -> ToolRouter -> StepContext -> ResponseItem` 统一链路，含 MCP 与 Skill 分支 |
| [03-MCP协议详细设计.md](04-工具与扩展生态/03-MCP协议详细设计.md) | MCP Host/Client：分层配置、连接池、tools/list、动态 runtime、原生 namespace、Step 路由、审批与输出预算 |
| [04-PluginSDK开发者文档.md](04-工具与扩展生态/04-PluginSDK开发者文档.md) | Plugin SDK：Host API 参考（astro_*）、自定义工具/Hook/Skill 开发、测试调试、打包发布、完整示例 |
| [05-Responses-API原生工具协议与Astro工具协议详细设计.md](04-工具与扩展生态/05-Responses-API原生工具协议与Astro工具协议详细设计.md) | Responses API 原生 Function/Freeform/Namespace/ToolSearch/WebSearch 契约与 Astro 工具协议：Direct / CodeModeOnly 分层投影、QuickJS `exec/wait`、TypeScript 工具声明与 Responses 回放 |

## 05-桌面端与交互

| 文件 | 说明 |
| ---- | ---- |
| [01-Tauri桌面端详细设计.md](05-桌面端与交互/01-Tauri桌面端详细设计.md) | 内嵌 agent-server、Thread RPC、Op 提交、rollout/live 投影与 HITL 宿主边界 |
| [02-前端组件详细设计.md](05-桌面端与交互/02-前端组件详细设计.md) | React 前端组件树、流式 Token 渲染、ApprovalDialog、Zustand 状态管理 |
| [03-对话分支系统设计.md](05-桌面端与交互/03-对话分支系统设计.md) | 对话分支：parent_conversation_id 两字段模型、create_branch、递归 CTE 分支树 |
| [04-全局搜索系统设计.md](05-桌面端与交互/04-全局搜索系统设计.md) | F-17 全局搜索：FTS5 消息搜索、文件实时扫描、session_search 工具、SearchModal UI |
| [05-对话公开分享系统设计.md](05-桌面端与交互/05-对话公开分享系统设计.md) | F-25 对话公开分享：媒体感知打包（HTML/ZIP）、SharePackager、系统原生分享框、P2 云端链接 |
| [06-对话管理详细设计.md](05-桌面端与交互/06-对话管理详细设计.md) | F-06 对话管理：ConversationRepository CRUD、生命周期状态机、标题自动生成、消息持久化、导入导出、侧边栏组件 |
| [07-多工作区管理详细设计.md](05-桌面端与交互/07-多工作区管理详细设计.md) | F-01 多工作区：WorkspaceRepository CRUD、数据隔离、配置管理（模型/Persona/工具）、模板系统、工作区切换 |
| [08-导出与分享详细设计.md](05-桌面端与交互/08-导出与分享详细设计.md) | F-25 导出：Markdown/PDF/JSON 多格式导出、选择性导出、媒体附件处理、内容脱敏、批量导出与工作区备份 |
| [09-国际化详细设计.md](05-桌面端与交互/09-国际化详细设计.md) | F-20 国际化：i18next 前端 + fluent-rs 后端、Prompt 多语言模板、日期数字格式化、运行时语言切换 |
| [10-快捷键与命令面板详细设计.md](05-桌面端与交互/10-快捷键与命令面板详细设计.md) | F-21 快捷键：ShortcutManager + useHotkeys、命令面板模糊搜索、自定义快捷键、上下文感知、无障碍性 |
| [11-设置与补充功能详细设计.md](05-桌面端与交互/11-设置与补充功能详细设计.md) | 设置界面（SettingsManager 分层配置、Provider 配置页、主题系统）+ OpenRouter 模型目录同步 + OCR 截屏识别 |
| [12-工作流编辑器详细设计.md](05-桌面端与交互/12-工作流编辑器详细设计.md) | 29 种节点/6 类能力的 DAG 引擎、变量解析、执行持久化及与 Harness 的边界 |
| [13-工作空间文件管理详细设计.md](05-桌面端与交互/13-工作空间文件管理详细设计.md) | F-10 文件管理：FileBrowser 组件、ai-docs 文档管理、文件预览、Agent 文件操作集成、存储配额 |
| [14-多模态交互详细设计.md](05-桌面端与交互/14-多模态交互详细设计.md) | 多模态交互：图片/语音/视频/文件输入输出、ContentPart 统一消息模型、语音对话模式、上下文预算管理 |
| [15-桌面宠物详细设计.md](05-桌面端与交互/15-桌面宠物详细设计.md) | 静态桌宠与 Codex-compatible v2 动画 atlas 的导入校验、Canvas 播放、Agent 状态映射和透明 Tauri 窗口 |

## 06-安全与基础设施

| 文件 | 说明 |
| ---- | ---- |
| [01-数据库访问层详细设计.md](06-安全与基础设施/01-数据库访问层详细设计.md) | Repository 模式：每表一个专属 Repository struct（ConversationRepo/MessageRepo/MemoryRepo 等），软替换、版本查询、Weibull 扫描封装 |
| [02-人工接管与授权详细设计.md](06-安全与基础设施/02-人工接管与授权详细设计.md) | Permission enum、PermissionSet、HumanGuard 状态机（L1/L2/L3）、白名单快速通行、审计日志、Prompt 注入防御 |
| [03-异步任务与通知系统设计.md](06-安全与基础设施/03-异步任务与通知系统设计.md) | F-15/F-24 异步任务 + 通知：媒体轮询器、定时任务 Scheduler、通知中心 UI、桌面系统通知 |
| [04-安全边界详细设计.md](06-安全与基础设施/04-安全边界详细设计.md) | F-09 安全边界：PathGuard/DomainGuard/ShellGuard、提示注入四层防护、子Agent资源限制、审计日志、威胁模型 |
| [05-可观测性详细设计.md](06-安全与基础设施/05-可观测性详细设计.md) | F-13 可观测性：结构化日志、SpanTree 请求追踪、Token/费用追踪、性能指标收集、开发者面板 UI |
| [06-错误处理详细设计.md](06-安全与基础设施/06-错误处理详细设计.md) | F-14 错误处理框架：统一错误类型层次、传播链、ErrorCategory 恢复策略、用户友好消息、优雅降级、Panic 防护 |
| [07-隐私与合规详细设计.md](06-安全与基础设施/07-隐私与合规详细设计.md) | F-10 隐私与合规：SensitiveFilter PII 过滤、Provider 数据声明、数据保留策略、用户数据导出/删除、GDPR 合规清单 |
| [08-数据迁移详细设计.md](06-安全与基础设施/08-数据迁移详细设计.md) | US-101 数据迁移：MigrationRunner、在线大表迁移、Rust 数据转换迁移、自动备份、版本兼容性管理 |
| [09-存储层详细设计.md](06-安全与基础设施/09-存储层详细设计.md) | 存储层：~/.astro/ 目录规范、StorageManager、Blob 内容寻址存储、缓存管理、临时文件、存储配额、清理策略 |
| [10-权限系统改进详细设计.md](06-安全与基础设施/10-权限系统改进详细设计.md) | 工具可见性与授权分离、StepContext、HITL、workspace grant、sandbox/network 的逐层权限模型 |

## 07-工程管理

| 文件 | 说明 |
| ---- | ---- |
| [01-需求设计追踪矩阵.md](07-工程管理/01-需求设计追踪矩阵.md) | F-01~F-39 / US-001~US-105 全量映射表：需求 → 设计文档章节双向索引，含覆盖度分析 |
| [02-开发者快速上手指南.md](07-工程管理/02-开发者快速上手指南.md) | 新成员入门：环境初始化、项目结构、新增 Tool/Provider/Command 步骤、调试方法、常见问题 |
| [03-测试策略详细设计.md](07-工程管理/03-测试策略详细设计.md) | 测试策略：单元测试 Mock 框架、LLM 录制回放、集成测试、E2E 测试、覆盖率目标、CI 测试矩阵 |
| [04-性能调优详细设计.md](07-工程管理/04-性能调优详细设计.md) | 性能调优：冷启动 <2s 优化、前端虚拟滚动、SQLite 查询调优、流式响应优化、内存管理、回归检测 |
| [05-CICD与发布详细设计.md](07-工程管理/05-CICD与发布详细设计.md) | CI/CD：GitHub Actions 工作流、跨平台构建矩阵、Eval 门控、发布通道（nightly/beta/stable）、代码签名、回滚策略 |
| [06-更新机制详细设计.md](07-工程管理/06-更新机制详细设计.md) | F-27 更新机制：Tauri Updater、增量下载与断点续传、插件兼容性检查、崩溃回滚、强制更新 |

## _v0.3规划（归档）

以下详细设计已归档至 `_v0.3规划/` 目录，待 v0.3 里程碑启动时激活。系统设计阶段的概要描述保留不变。

| 文件 | 原位置 | 说明 |
| ---- | ---- | ---- |
| 自我进化引擎详细设计.md | 01-核心引擎层 | DSPy 风格 Prompt 优化、A/B 验证、跨层蒸馏 |
| WASM插件沙箱API设计.md | 04-工具与扩展生态 | wasmtime 运行时、Host Functions、Plugin SDK |
| 工作流DAG执行引擎设计.md | 04-工具与扩展生态 | Kahn 并发调度、节点类型、暂停/恢复 |
| Agent市场详细设计.md | 04-工具与扩展生态 | .agent 包格式、安装流程、评分系统 |
| 工作流触发器详细设计.md | 04-工具与扩展生态 | Cron/Webhook/Event/Chain 触发 |
| E2E加密同步详细设计.md | 06-安全与基础设施 | XChaCha20 加密、增量同步、多设备配对 |
| Agent评估系统设计.md | 06-安全与基础设施 | LLM-as-Judge、回归检测、CI 门控 |

---

> 合并说明：02-agent-runtime详细设计 由原 02 和 34 合并；01-Skills系统详细设计 由原 05 和 45 合并；43-用户建模系统设计 已被记忆系统 L5 层取代，归档至 _历史参考/
