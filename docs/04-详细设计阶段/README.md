# 详细设计阶段文档索引

> 阶段：详细设计 | 说明：本目录是系统设计的深化，包含具体算法、数据结构、代码级实现方案

---

## 01-核心引擎层

| 文件 | 说明 |
| ---- | ---- |
| [01-agent-core详细设计.md](01-核心引擎层/01-agent-core详细设计.md) | agent-core Crate 完整实现：核心数据类型（AgentContext/Message/ToolCall）、run_agent_turn 编排循环、SQLite Repository 层、5 层记忆子系统、成本追踪 |
| [02-agent-runtime详细设计.md](01-核心引擎层/02-agent-runtime详细设计.md) | agent-runtime 完整设计（合并）：round_loop 执行循环、Planner 自适应规划、HumanGuard 审批流、WASM/Shell 沙箱、后台任务调度、多会话并发、重试限流 |
| [03-交互执行模式设计.md](01-核心引擎层/03-交互执行模式设计.md) | F-39/M-38 交互执行模式：自适应规划、Pending 消息队列、YOLO 开关、MultiTask |
| [04-多执行后端系统设计.md](01-核心引擎层/04-多执行后端系统设计.md) | F-15/M-08 多执行后端：ExecutionBackend trait、LocalShell/Docker/SSH 实现、BackendRegistry 路由 |
| [05-成本预算控制详细设计.md](01-核心引擎层/05-成本预算控制详细设计.md) | BudgetManager：三级预算模型、Token 计费、check_before_call 拦截、渐进降级、YOLO 强制关闭、费用统计报表 |
| [06-子Agent派生详细设计.md](01-核心引擎层/06-子Agent派生详细设计.md) | F-30 子Agent派生：Supervisor 模式、权限交集继承、资源隔离、并发控制、深度限制、agent_spawn 工具 |
| [07-Agent生命周期详细设计.md](01-核心引擎层/07-Agent生命周期详细设计.md) | Agent 生命周期：7 阶段状态机、AgentInstance 模型、SessionManager、会话持久化与恢复、崩溃恢复、配置热更新 |
| [08-Hooks系统详细设计.md](01-核心引擎层/08-Hooks系统详细设计.md) | Hooks 扩展系统：18 种生命周期事件、HookRegistry/Pipeline、6 个内置 Hook、用户自定义（Shell/WASM）、hooks.toml 配置 |
| [09-Checkpoint与状态快照详细设计.md](01-核心引擎层/09-Checkpoint与状态快照详细设计.md) | Checkpoint 机制：自动/手动快照、状态恢复（覆盖/分支）、工作流断点续传、增量存储、比较与回放 |
| [10-自主决策与行为进化详细设计.md](01-核心引擎层/10-自主决策与行为进化详细设计.md) | 自主进化：BehaviorEngine 行为策略、反馈信号收集、偏好学习模型、Prompt 自优化、工具模式学习、主动决策引擎 |
| [11-Agent协作协议详细设计.md](01-核心引擎层/11-Agent协作协议详细设计.md) | Agent 协作：Debate/MapReduce/Voting 三种协议、AgentRole 角色定义、MessageBus 跨 Agent 通信、资源预算分配 |

## 02-Provider与模型层

| 文件 | 说明 |
| ---- | ---- |
| [01-agent-providers详细设计.md](02-Provider与模型层/01-agent-providers详细设计.md) | agent-providers 各 Provider 独立客户端实现（Anthropic/OpenAI/DeepSeek/MiniMax 等）、ProviderRegistry 路由与 fallback、流式响应处理 |
| [02-多模型对比系统设计.md](02-Provider与模型层/02-多模型对比系统设计.md) | F-33/US-082 多模型对比：tokio 并发流式调用、CompareView 并排 UI、选优采纳、费用独立统计 |
| [03-Provider故障转移设计.md](02-Provider与模型层/03-Provider故障转移设计.md) | F-14 故障转移：错误可重试性分类、指数退避+Jitter、熔断器、Provider 自动切换、流式续传、全链路降级 |
| [04-离线与本地模型详细设计.md](02-Provider与模型层/04-离线与本地模型详细设计.md) | F-18 离线能力：Ollama Sidecar 生命周期、本地模型发现与注册、NetworkMonitor 离线检测、自动降级切换、在线恢复 |
| [05-模型洞察界面详细设计.md](02-Provider与模型层/05-模型洞察界面详细设计.md) | F-26 模型洞察：使用统计、性能排行榜、成本分析与投影、模型推荐引擎、数据聚合优化 |

## 03-记忆与上下文

| 文件 | 说明 |
| ---- | ---- |
| [01-记忆系统详细设计.md](03-记忆与上下文/01-记忆系统详细设计.md) | 5 层记忆架构：情节/语义/持久/程序性/用户建模，KNN 向量检索、Weibull 衰减遗忘、MEMORY.md 快照、BM25 技能召回 |
| [02-上下文压缩算法设计.md](03-记忆与上下文/02-上下文压缩算法设计.md) | F-07 上下文压缩：三级策略（滑动窗口/LLM摘要/紧急丢弃）、消息重要性评分、Prompt Cache 保护 |
| [03-Persona与SystemPrompt注入设计.md](03-记忆与上下文/03-Persona与SystemPrompt注入设计.md) | F-11 Agent 人格：8 槽位注入顺序、SOUL.md 规范、/personality 预设库、SystemPromptBuilder |
| [04-知识库RAG详细设计.md](03-记忆与上下文/04-知识库RAG详细设计.md) | F-05 知识库RAG：文档导入管道、RecursiveChunker 分块算法、BM25+向量混合检索（RRF）、knowledge_manage 工具 |
| [05-记忆图谱详细设计.md](03-记忆与上下文/05-记忆图谱详细设计.md) | 记忆图谱：实体/关系抽取、SQLite 图查询（递归 CTE）、图谱增强检索、实体去重、vis-network 可视化 |
| [_历史参考/用户建模系统设计.md](03-记忆与上下文/_历史参考/用户建模系统设计.md) | ⚠️ 已废弃 — 原用户建模设计，已被 01-记忆系统详细设计 的 L5 层取代，仅保留历史参考 |

## 04-工具与扩展生态

| 文件 | 说明 |
| ---- | ---- |
| [01-Skills系统详细设计.md](04-工具与扩展生态/01-Skills系统详细设计.md) | Skills 系统（对齐 Claude Code）：目录结构、动态上下文注入(!`cmd`)、$ARGUMENTS 参数、多级发现、调用控制、subagent 执行、Skill Hooks、BM25 召回、进化引擎、版本管理 |
| [02-工具系统详细设计.md](04-工具与扩展生态/02-工具系统详细设计.md) | 工具系统：名词_动词命名规范、4 个原子批量工具（file_edit/memory_manage/skill_manage/knowledge_manage）、RiskLevel 风险分级、ToolOutput 统一格式 |
| [03-MCP协议详细设计.md](04-工具与扩展生态/03-MCP协议详细设计.md) | Codex 对齐的 MCP Host/Client：STDIO/Streamable HTTP、分层配置、认证与 OAuth、Server Instructions、工具审批、连接状态机、迁移与验收 |
| [04-PluginSDK开发者文档.md](04-工具与扩展生态/04-PluginSDK开发者文档.md) | Plugin SDK：Host API 参考（astro_*）、自定义工具/Hook/Skill 开发、测试调试、打包发布、完整示例 |
| [05-Codex原生工具协议与CodeMode详细设计.md](04-工具与扩展生态/05-Codex原生工具协议与CodeMode详细设计.md) | Codex 工具协议对齐：Function/Freeform/Namespace/ToolSearch/WebSearch、Deferred MCP 激活、Responses 回放、Code Mode 与三态暴露策略 |

## 05-桌面端与交互

| 文件 | 说明 |
| ---- | ---- |
| [01-Tauri桌面端详细设计.md](05-桌面端与交互/01-Tauri桌面端详细设计.md) | Tauri 桌面端 AppState 依赖注入容器（Arc RwLock）：ProviderRegistry/ToolRegistry/SkillRegistry/SessionManager/HumanGuard 初始化与生命周期 |
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
| [12-工作流编辑器详细设计.md](05-桌面端与交互/12-工作流编辑器详细设计.md) | F-08 工作流编辑器：React Flow 画布、18 种节点类型、AI 辅助创建（NL→DAG）、运行可视化、变量模板、Skill 编译 |
| [13-工作空间文件管理详细设计.md](05-桌面端与交互/13-工作空间文件管理详细设计.md) | F-10 文件管理：FileBrowser 组件、ai-docs 文档管理、文件预览、Agent 文件操作集成、存储配额 |
| [14-多模态交互详细设计.md](05-桌面端与交互/14-多模态交互详细设计.md) | 多模态交互：图片/语音/视频/文件输入输出、ContentPart 统一消息模型、语音对话模式、上下文预算管理 |

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
