# 系统设计阶段文档索引

> 阶段：系统设计 | 说明：从需求分析阶段承接，描述 Astro Agent 各模块的技术架构、实现方案与接口设计

---

## 目录结构

```text
03-系统设计阶段/
├── 01-架构设计/           # 架构总览、Crate 结构、系统分层、模块依赖、数据流、部署、端到端追踪、ADR
├── 02-核心功能模块/       # Provider、MCP/Skills/Subagent、进化引擎、上下文、RAG
├── 03-基础设施/           # 持久化、可观测性、错误处理、安全、隐私、成本
├── 04-数据库设计/         # Schema、ER 图、索引、迁移策略
├── 05-接口设计/           # Tauri Commands、DTO、MCP 协议、Provider 接口
├── 06-桌面端/             # Tauri、国际化、快捷键、工作流、多工作区、离线、导出
├── 07-UI设计/             # 主界面、交互流程、设置界面、组件规范
├── 08-质量保障/           # 测试策略、性能调优、CI/CD、更新、备份
└── 09-生态扩展/           # 插件生态、Agent 市场
```

---

## 01-架构设计

| 文件 | 说明 |
| ---- | ---- |
| [01-架构总览.md](01-架构设计/01-架构总览.md) | 架构总览、技术栈选型、Monorepo 整体结构 |
| [02-项目初始化.md](01-架构设计/02-项目初始化.md) | 项目初始化步骤与各模块开发顺序 |
| [03-Crate结构.md](01-架构设计/03-Crate结构.md) | 各 Rust crate 详细结构与核心 trait 定义 |
| [04-Prompt工程.md](01-架构设计/04-Prompt工程.md) | Prompt scaffold：稳定基础指令、角色化动态上下文和独立原生工具 schema |
| [05-系统分层架构.md](01-架构设计/05-系统分层架构.md) | 物理分层与纵向 Agent Harness 职责面 |
| [06-模块依赖关系.md](01-架构设计/06-模块依赖关系.md) | Crate 依赖图、React 组件模块划分、外部依赖清单 |
| [07-数据流设计.md](01-架构设计/07-数据流设计.md) | 主对话 / 工具调用 / 记忆读写 / Skills 执行 / 子 Agent 派生数据流 |
| [08-部署架构.md](01-架构设计/08-部署架构.md) | 本地单机部署、多平台打包、自动更新、Ollama sidecar |
| [09-端到端数据流追踪.md](01-架构设计/09-端到端数据流追踪.md) | 端到端数据流追踪：从用户输入到 LLM 响应的全链路数据流 |
| [10-架构决策记录ADR.md](01-架构设计/10-架构决策记录ADR.md) | 架构决策记录（ADR）：关键技术选型与设计决策的记录与追溯 |
| [11-Agent-Harness总体架构.md](01-架构设计/11-Agent-Harness总体架构.md) | Agent = Model + Harness；统一执行循环、上下文、工具、安全、恢复、观测和运行环境边界 |
| [12-Responses原生Agent运行时架构.md](01-架构设计/12-Responses原生Agent运行时架构.md) | Agent Responses-only 路由、canonical `ResponseItem`、生命周期、Hooks 与 rollout 恢复边界 |

## 02-核心功能模块

| 文件 | 说明 |
| ---- | ---- |
| [01-多模态Provider.md](02-核心功能模块/01-多模态Provider.md) | 多模态 Provider 系统：8 个 trait（含 MultimodalClient 继承 TextClient、ImageClient / MusicClient）、路由与降级 |
| [02-OpenRouter模型目录.md](02-核心功能模块/02-OpenRouter模型目录.md) | OpenRouter 模型目录：价格、能力、模态，实时数据同步方案 |
| [03-MCP集成.md](02-核心功能模块/03-MCP集成.md) | MCP Client、STDIO/Streamable HTTP、Hub 连接池、Registry runtime、Step 路由与审批/输出链 |
| [04-Skills系统.md](02-核心功能模块/04-Skills系统.md) | SKILL.md 发现与加载、`skills` 工具、`astro_tools` additive gate 和 Step 冻结 |
| [05-Subagent系统设计.md](02-核心功能模块/05-Subagent系统设计.md) | Subagent / Codex V2 Agent Threads：六工具控制面、Graph/mailbox/status 持久化、恢复与权限收窄 |
| [06-自我进化引擎.md](02-核心功能模块/06-自我进化引擎.md) | 自我进化引擎：TaskTrace、反思引擎、Skill 合成、Prompt 优化流程 |
| [07-上下文管理.md](02-核心功能模块/07-上下文管理.md) | PromptContract、动态上下文、工具结果压缩、token 预算与用量分段 |
| [08-知识库RAG.md](02-核心功能模块/08-知识库RAG.md) | 知识库 RAG：文档摄入管线、混合检索（BM25 + sqlite-vec）、工作区独立知识库 |
| [09-会话Checkpoint与恢复.md](02-核心功能模块/09-会话Checkpoint与恢复.md) | append-only rollout 事实源、snapshot + live boundary、SessionStore 投影与恢复 |

## 03-基础设施

| 文件 | 说明 |
| ---- | ---- |
| [01-持久化层.md](03-基础设施/01-持久化层.md) | rollout 事实源 + Session/Agent Graph/Usage/Cron/Artifact/Knowledge 独立投影存储 |
| [02-可观测性.md](03-基础设施/02-可观测性.md) | 可观测性：结构化日志、OpenTelemetry 追踪、CostRecord 成本追踪 |
| [03-错误处理与容错.md](03-基础设施/03-错误处理与容错.md) | 错误处理与容错：错误分类体系、重试退避、Provider 自动切换 |
| [04-人工接管设计.md](03-基础设施/04-人工接管设计.md) | interaction mode、HitlGate、PauseControl、SessionApprovalCache 与协议控制事件 |
| [05-工具系统设计.md](03-基础设施/05-工具系统设计.md) | Registry/StepContext/ToolRouter、原生工具协议、Deferred 发现与执行闭环 |
| [06-安全边界.md](03-基础设施/06-安全边界.md) | 暴露、快照、审批、attempt-scoped sandbox/network、hooks 与审计多层边界 |
| [07-隐私与合规.md](03-基础设施/07-隐私与合规.md) | 隐私与合规：敏感词过滤、Provider 数据声明、SQLCipher 加密方案 |
| [08-成本预算控制.md](03-基础设施/08-成本预算控制.md) | 成本预算控制：全局 / 工作区 / 任务预算层级、超支告警、费用仪表板 |

## 04-数据库设计

| 文件 | 说明 |
| ---- | ---- |
| [01-Schema设计.md](04-数据库设计/01-Schema设计.md) | **权威 Schema**：17 张表 + 2 虚拟表 + 2 视图完整 DDL |
| [02-ER关系图.md](04-数据库设计/02-ER关系图.md) | Mermaid ER 图（18 个实体：17 张表 + embeddings 虚拟表）、聚合根识别、工作区隔离设计 |
| [03-索引设计.md](04-数据库设计/03-索引设计.md) | B-tree 索引、FTS5 全文索引（含 jieba 中文分词）、查询性能目标 |
| [04-数据迁移策略.md](04-数据库设计/04-数据迁移策略.md) | 版本化迁移（sqlx）、回滚策略、SQLCipher 迁移处理 |

## 05-接口设计

| 文件 | 说明 |
| ---- | ---- |
| [01-Tauri-Commands-API.md](05-接口设计/01-Tauri-Commands-API.md) | 58 个 Tauri Commands + 7 种流式事件 |
| [02-前后端数据结构.md](05-接口设计/02-前后端数据结构.md) | 核心 DTO（TypeScript + Rust serde）、枚举、命名规范 |
| [03-MCP协议接口.md](05-接口设计/03-MCP协议接口.md) | JSON-RPC 2.0 消息格式、客户端 / 服务端方法、连接生命周期 |
| [04-Provider接口规范.md](05-接口设计/04-Provider接口规范.md) | 8 个核心 trait（含 MultimodalClient 继承 TextClient）、ProviderError、流式响应、Fallback 策略 |

## 06-桌面端

| 文件 | 说明 |
| ---- | ---- |
| [01-桌面端Tauri.md](06-桌面端/01-桌面端Tauri.md) | Tauri v2 桌面端架构：React + TypeScript 前端、事件驱动 |
| [02-国际化.md](06-桌面端/02-国际化.md) | 国际化方案：react-i18next、Prompt 多语言、locale 切换 |
| [03-快捷键与命令面板.md](06-桌面端/03-快捷键与命令面板.md) | 快捷键与命令面板：Cmd+K 实现、全局快捷键 |
| [04-工作流编辑器.md](06-桌面端/04-工作流编辑器.md) | 工作流可视化编辑器：React Flow 节点设计、编译为 Skill |
| [05-多工作区隔离.md](06-桌面端/05-多工作区隔离.md) | 多工作区隔离：配置 / 记忆 / Skill / MCP 独立存储、预设模板 |
| [06-离线能力.md](06-桌面端/06-离线能力.md) | 离线能力：Ollama sidecar、网络检测、优雅降级 |
| [07-导出与分享.md](06-桌面端/07-导出与分享.md) | 对话导出与分享：Markdown / PDF / HTML 导出、脱敏处理 |
| [08-模型洞察界面.md](06-桌面端/08-模型洞察界面.md) | Model Insights：多维排行榜、筛选器、模型对比交互 |
| [09-补充功能设计.md](06-桌面端/09-补充功能设计.md) | 补充设计：F-26 OCR 截图识别、F-28 专注阅读模式、F-35 多执行后端 |

## 07-UI设计

| 文件 | 说明 |
| ---- | ---- |
| [01-主界面设计.md](07-UI设计/01-主界面设计.md) | 主界面布局线框图（侧边栏 / 对话区 / 右侧面板 / 顶部栏） |
| [02-关键界面交互流程.md](07-UI设计/02-关键界面交互流程.md) | 发送消息、人工审批、Skills 浏览、工作区切换等核心流程 |
| [03-设置界面设计.md](07-UI设计/03-设置界面设计.md) | Provider 配置、预算控制、MCP 管理、隐私设置、数据管理 |
| [04-组件设计规范.md](07-UI设计/04-组件设计规范.md) | 设计系统基础、原子 / 复合 / 布局组件、动画规范 |
| [05-主题系统设计.md](07-UI设计/05-主题系统设计.md) | Glassmorphism 玻璃拟态、亮/暗/自动、Shell 渐变与本地/AI 壁纸 |

## 08-质量保障

| 文件 | 说明 |
| ---- | ---- |
| [01-测试策略.md](08-质量保障/01-测试策略.md) | 测试策略：Mock 方案、录制回放、进化回归、Eval 套件、前端 E2E |
| [02-性能调优.md](08-质量保障/02-性能调优.md) | 性能调优：冷启动优化、虚拟滚动、SQLite 索引、内存 LRU 缓存 |
| [03-CICD与发布.md](08-质量保障/03-CICD与发布.md) | CI/CD 与发布流水线：GitHub Actions、多平台构建、Eval 门控 |
| [04-更新机制.md](08-质量保障/04-更新机制.md) | 更新机制：Tauri Updater、SQLite 迁移、插件兼容检查 |
| [05-备份与同步.md](08-质量保障/05-备份与同步.md) | 数据备份与同步：本地备份策略、E2E 加密云同步、导入导出 |

## 09-生态扩展

| 文件 | 说明 |
| ---- | ---- |
| [01-插件生态.md](09-生态扩展/01-插件生态.md) | 插件生态：WASM 插件市场、MCP Server 目录、权限映射 |
| [02-Agent市场.md](09-生态扩展/02-Agent市场.md) | Agent 市场：.agent 包格式、安装流程、官方精选目录 |

---

> 详细设计文档见：[04-详细设计阶段/](../04-详细设计阶段/)
>
> 术语统一规范见：[05-质量审查阶段/02-术语统一规范.md](../05-质量审查阶段/02-术语统一规范.md)
