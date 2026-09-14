# Codex 上游增量与 Astro 对齐报告：Step 设置、线程附件与 Worktree

## 执行摘要

- 执行时间：2026-09-14 09:16 CST（Asia/Shanghai）。
- Codex 仓库：`/Users/iswm/CodeRope/codex`，分支 `main`。
- 拉取结果：工作树原本干净，`git pull --ff-only` 成功 fast-forward；拉取后与 `origin/main` 同步且工作树干净。
- CodeGraph：Astro 使用既有索引；Codex 初查时尚未初始化，收到明确指示后执行 `codegraph init`，索引 5,126 个文件、158,172 个节点、582,074 条边，并用 `codegraph explore` 复核 Step settings、thread attachments、worktree 与 credential/tool metadata 链路。
- 精确范围：`956aa3f6372ffd73a0df639beff356b3b664b858..3abbf9fe2c6b6910e9de61f6a0c5bb468f74b5c8`。
- 增量规模：462 个提交、2,289 个文件、`+199,475/-54,506`。
- Astro 对照分支：`feature/diagnostics-ui-redesign`。报告写入前存在 13 个已跟踪改动与 12 个未跟踪路径，均为其他任务内容；本次只新增并提交本报告。
- 核心结论：本轮应优先把 Astro 的 provider/model/approval/tool/hook 选择纳入同一份不可变 Step 快照；其次补齐 thread attachment 的持久 CRUD/通知契约与 worktree 清单、owner、恢复/安全删除控制面。MCP elicitation 的取消与陈旧响应防护、模型缓存按配置/凭证失效已经基本对齐。

## 拉取结果与 Git 证据

| 项目 | 结果 |
|---|---|
| 拉取前 HEAD / 成功基线 | `956aa3f6372ffd73a0df639beff356b3b664b858` |
| 拉取后 HEAD / `FETCH_HEAD` | `3abbf9fe2c6b6910e9de61f6a0c5bb468f74b5c8` |
| Git 范围 | `956aa3f6372ffd73a0df639beff356b3b664b858..3abbf9fe2c6b6910e9de61f6a0c5bb468f74b5c8` |
| ancestry | `git merge-base --is-ancestor` 成功，确认纯前向范围 |
| 提交数 | `git rev-list --count` = 462 |
| 第一条新增提交 | `f46671b14a`，2026-09-04T01:35:17Z，Render assistant file citations as local links |
| 最后一条新增提交 | `3abbf9fe2c`，2026-09-14T00:29:16Z，Extract Windows sandbox configuration preparation into a helper |
| diff 统计 | 2,289 files changed, 199,475 insertions, 54,506 deletions |

提交日期分布按 Git author ISO 日期统计：09-04 53、09-05 38、09-06 17、09-07 62、09-08 62、09-09 89、09-10 61、09-11 46、09-12 20、09-13 13、09-14 1。提交数是提交数，不是分支数。本报告只使用本地 Git 范围、提交和当前源码，不以 changelog 代替证据。

## 按时间顺序的新提交内容

### 2026-09-04（53）

主线是 managed worktree、Guardian 请求级审批与 retained context、异步问题组件、MCP catalog 热刷新和 voice transport。代表提交：`eb5a00b068`（exec worktree）、`0ae02915bd`（request-scoped Guardian decision）、`218e8df926`（异步问题进入 TUI）、`32351a7b1a`/`2cfee7de25`（MCP client/catalog 刷新）。

### 2026-09-05（38）

异步问题补齐 selectable/Other/队列状态，交互与 fork 开始绑定稳定 root turn/runtime identity；interactive session 也进入 managed worktree。代表提交：`07f18d5ff7`、`c126b0d8ef`、`be2684ede0`、`d05e6d5f46`、`3525845978`、`f6976ab036`。

### 2026-09-06（17）

voice host 增长较多；与 Astro 更相关的是 context 能力按模型 gate、legacy resume 迁移保护、active writer 冲突时只读查看、worktree 发现与浏览。代表提交：`6af345407d`、`ac192cd793`、`4aec23384e`、`d30f9cc72a`、`b053ef9e5a`。

### 2026-09-07（62）

app-server/TUI 开始系统化支持 user verification、MCP elicitation 统一审批、worktree owner、fork 历史选择以及 Guardian context registry。代表提交：`ad931a45b2`、`3cd6004dc4`、`555b82afa9`、`ce5c4133bd`、`5b85aea979`、`9f70e348e0`。

### 2026-09-08（62）

macOS Secure Enclave user verification 从 provider、RPC 到 TUI 接通；memory v2 引入独立存储、抽取/合并和 dual-write readiness；同时收紧 shell snapshot、模型 catalog cache、网络代理 teardown，并落地 thread attachment 的事务存储与 worktree owner/确认删除。代表提交：`e7637306bc`、`2cbbf0c9b5`、`f31bd3adff`、`2e220af1f6`、`589874be81`、`973dcd80fc`。

### 2026-09-09（89）

本轮最关键的一天：credential broker 增加可配置 provider、别名、上下文保留和 HTTP tunnel；rollout compression 与 active writer 协调；Step 设置开始贯穿 model context、extension、tool plan/execution；并加入工具结果 metadata、线程附件分页、daemon 恢复和 MCP reconnect/cancellation 修复。代表提交：`1bfd383890`、`634ebc1865`、`73a1148c9c`、`b4507997e0`、`8ff4aa8ee4`、`205f3671e1`、`0df6366a87`、`130d6e4fba`、`e1b23086ac`。

### 2026-09-10（61）

线程附件完成 app-server add/list/remove/notification 链；native verification 进入 MCP continuation 与 Desktop；Guardian settings、预算和 evidence 进一步快照化；MCP 状态暴露 OAuth failure，provider/API key model discovery 和 thread/plugin settings 扩展。代表提交：`2df0b747ba`、`3319d9b296`、`c6a59ef923`、`6bb5be869f`、`d996b4f02a`、`ea53c8d4f7`。

### 2026-09-11（46）

权限/配置解析更强调 execution host 与 folder consent；MCP 暴露 server capabilities 和 enterprise auth；code-mode callback 绑定单次 execution；worktree 默认开启。另有大量 Windows MXC、voice 与 TUI 变化。代表提交：`02a8f038b8`、`7a6f469dcf`、`654b0a77d0`、`3305c4f31d`、`68bc5369ba`。

### 2026-09-12（20）

context snapshot 场景覆盖、任务列表只读打开/用量展示、recap 上下文保留与 token 估算改进；同时移除 TUI personality 选择并继续 release 管道整理。代表提交：`89c8bcf37d`、`7efb0262d6`、`aee8a55ab6`、`8d3c6cc13d`、`b04a2c2645`。

### 2026-09-13（13）

Windows MXC 进入真实 command execution；direct tool-call metadata 与输出绑定；Step 快照继续覆盖 request metadata/tool hooks；agents overview 可直接创建 worktree session。代表提交：`c379459bba`、`1715e55076`、`16537b20a5`、`6f39a47bb3`。

### 2026-09-14（1）

`3abbf9fe2c` 抽取 Windows sandbox 配置准备 helper，属于平台实现收口，不改变 Astro 当前优先级。

## 关键 Codex 源码证据

| 主题 | 提交与当前源码 | 关键语义 |
|---|---|---|
| Step 不可变设置 | `b4507997e0`、`8ff4aa8ee4`、`205f3671e1`、`6bb5be869f`、`16537b20a5`；`codex-rs/core/src/session/step_context.rs:17-36`、`step_settings.rs:21-52` | 单次 sampling 先捕获 `ResolvedStepSettings`，同一快照用于模型信息、reasoning、service tier、approval policy、MCP、tool plan/execution、hooks、telemetry 与持久 context item；live update 只影响下一 step。 |
| MCP elicitation 生命周期 | `3436cad5ab`；`codex-rs/rmcp-client/src/elicitation_client_service.rs`、`streamable_http_retry.rs` | 连接取消/重连时清理 pending elicitation，避免旧请求或旧 sender 污染新连接。 |
| Thread attachment 契约 | `589874be81`、`130d6e4fba`、`2df0b747ba`、`3319d9b296`；`app-server-protocol/src/protocol/v2/thread_attachment.rs:9-106`、`app-server/src/request_processors/thread_attachments.rs:35-181` | `threadId + attachmentType + identityKey` 幂等关联；持久层事务 add/remove、cursor 分页 list，先响应提交结果再广播 created/deleted notification，并设置类型、identity、payload、页大小上限。 |
| Worktree 生命周期 | `3525845978`、`973dcd80fc`、`68bc5369ba`、`6f39a47bb3`；`codex-rs/worktree/src/lib.rs:168-328`、`tui/src/worktree_browser.rs:20-167` | 枚举时校验 managed layout、common dir、gitdir backlink；单独绑定 owner thread；UI 区分可恢复/已归档/未知 owner；删除拒绝当前 checkout、脏树与 ignored local files。 |
| 凭证代理 | `1bfd383890`、`5a9aec40a5`、`f45115a137`、`38cbebaf3f`、`634ebc1865`、`ed4ca07ba6`；`network-proxy/src/credential_broker/provider_config.rs:5-28`、`environment.rs:21-103`、`mitm.rs:242-298` | 将真实 secret 留在 broker，子进程只拿替身/绑定信息；按 URL prefix、auth method 和目标注入，环境过滤后仍保留私有路由上下文，并支持明文 HTTP tunnel 的显式危险开关。 |
| Rollout writer/daemon 恢复 | `73a1148c9c`、`ce2c2759eb`、`7c88f037d9`、`e1b23086ac`；`rollout/src/writer_lock.rs`、`thread-store/src/local/live_writer.rs`、`app-server/src/daemon_thread_recovery.rs` | compression 与活跃 writer 协调；启动取消释放 writer；daemon 关闭记录恢复候选，重启恢复已加载线程。 |
| Tool result metadata | `0df6366a87`、`1715e55076`；`protocol/src/models/executed_tool_calls.rs`、`core/src/tools/executed_tool_calls.rs` | tool result metadata 有总量上限和 oversized 标记，绑定到产生该结果的 direct invocation；host-only 字段在 app-server raw notification 前清除，避免泄露。 |
| Provider/cache | `f31bd3adff`、`f046cf35df`、`ea53c8d4f7`、`102fc57e4a` | model catalog cache 绑定 provider/auth identity；OpenAI API key 可选 model discovery；quota 与 rate limit 分型。 |
| Native user verification | `e7637306bc`、`82d4a98912`、`3715bf4100`、`7b491281c8` | macOS Secure Enclave 签名、RPC 取消、Desktop 激活和 public-key metadata 形成独立于普通 UI 审批的身份校验链。 |

## Astro 对齐矩阵

| 优先级 | 分类 | 结论 | Astro 当前证据与差距 | 推荐下一步 / 风险 |
|---|---|---|---|---|
| P0 | runtime / provider / tools / hooks | **建议对齐：完整 Step settings snapshot** | Astro `StepContext` 当前只含 `turn/history/prompt_context/tool_router`（`crates/agent-core/src/runtime/step_context.rs:7-28`）。`run_turn` 在 `capture_step_context()` 和构造 `sampling_prompt` 后才从可变 `TurnContext::provider_settings()` 切换 streamer（`streaming/multi_turn.rs:631-683`）；工具分发又从同一可变对象读取 `service_tier`（`runtime/tool_dispatch.rs:158-171`）。更新恰好落在边界时，一次 response 的 prompt/tool plan、实际 model、tool context/hook metadata 可能来自不同 generation。 | 引入 `ResolvedStepSettings`（或等价结构）并在 `capture_step_context` 一次性冻结 target/model config、model capabilities、reasoning/service tier、approval/reviewer 与 telemetry；prompt、provider request、tool dispatch、hooks、持久化 context item全部只读该快照。回归测试覆盖更新发生在 capture 后、request 前和 response 后/tool 前。风险是触及 core/provider/tool/hook 多层，应拆成“类型与捕获”“消费者迁移”“恢复与测试”三批。 |
| P1 | persistence / protocol / app-server / Tauri / React | **建议对齐：thread-scoped attachment CRUD** | Astro `ArtifactRow` 有 `session_id/message_id`，但 identity 是全局 `path`（`crates/agent-artifacts/src/db.rs:81-95,223-280`）；Tauri `save_chat_upload` 直接写 `uploads/<session>`，删除入口按 paths（`commands/workspace/artifacts.rs:276-333`）。Composer attachment 能发送，但没有共享 `ThreadAttachment` 类型、幂等 identity key、cursor list 或 committed mutation notification。 | 在 `agent-protocol` 定义 bounded attachment identity/payload；`agent-session` 或专用 store 建 `(thread,type,identity)` 唯一键和事务 CRUD；`agent-proto`/server/Tauri 暴露 add/list/remove + revisioned notification；React 从 durable list 恢复。不要替换 artifacts 索引：artifact 是文件目录，thread attachment 是线程状态，两者可通过 durable ref 关联。风险是删除的文件生命周期与引用计数必须明确。 |
| P1 | subagents / worktree / Desktop | **建议对齐：可恢复 worktree 清单与 owner** | Astro `WorktreeManager` 已安全创建、写 manifest、脏树拒绝自动清理（`crates/agent-delegate/src/lib.rs:83-215,405-438`），Tauri 已有 prepare/cleanup（`commands/agents/agent.rs:222-263`）；但没有可信 `list`/owner API，项目菜单仍提示“永久工作树功能仍在开发中”（`apps/desktop/src/App.tsx:2833-2838`）。 | 增加只读枚举并同时校验 bucket/manifest、Git common-dir 与 backlink；将 owner thread ID 原子绑定到 checkout；Desktop 展示 resumable/archived/orphan 状态，删除前显式确认并拒绝当前 checkout、tracked/untracked/ignored 变更。风险集中在 symlink/stale registration/并发启动，不能只遍历目录。 |
| P1 | tools / protocol / persistence / security | **建议对齐：有界且 host-only 的工具结果 metadata** | Astro `ResponseItem` 支持 passthrough metadata，但 `ToolOutput` 只有 Text/Media/FileChanges（`crates/agent-types/src/tool_output.rs:33-47`）；`record_tool_result_with_id_and_media` 只合成 `astro_media/status/file_changes`（`runtime/recording.rs:329-399`），MCP/direct tool 无法把受信 result metadata 绑定到对应 invocation。 | 给 `ToolOutput` 增加独立 metadata 通道，按 call ID 绑定并设置总字节上限/oversized marker；明确 provider-visible、rollout-only、UI-visible 三种投影，默认不把 host-only metadata发到 raw app-server/Tauri 事件。风险是 secret/PII 泄露和历史膨胀，必须先做边界与红线测试。 |
| P2 | sandbox / network | **条件建议：credential broker，而不是直接透传 secret** | Astro managed proxy 是 loopback HTTP/1 CONNECT，`prepare()` 覆盖代理变量但保留其余 env；README 明确 plain HTTP/SOCKS 未实现（`crates/agent-network-proxy/src/lib.rs:1-5`、`proxy.rs:102-145`）。当前没有 provider registry、dummy credential、URL-prefix 绑定或代理侧 header 注入。 | 仅在 managed proxy 要承载真实第三方 CLI 凭证时推进：先定义 secret-free child env 与 URL-prefix policy，再实现 HTTPS MITM/受限 HTTP forwarding，最后接 shell snapshot scrub/replay。不能在现有 blind CONNECT 上声称完成 credential injection；明文 HTTP 必须独立危险开关。 |
| P2 | persistence / recovery | **部分对齐：线程生命周期强，跨进程 writer ownership 弱** | Astro `ThreadManager` 有 per-thread creation lock、lease-aware idle unload，server 还有 generation/release ownership 与取消测试；rollout recorder 是单进程后台 writer。没有 Codex `writer_lock/live_writer` 对跨进程 active writer、压缩、归档/删除的统一裁决。 | 当前内嵌单 backend 可继续使用现有模型；若引入 daemon/多客户端/外部压缩器，再增加持久 writer identity、只读 attach 和启动取消释放测试。现在直接搬 daemon recovery 会引入无产品收益的复杂度。 |

## 已对齐

- **MCP elicitation 取消与陈旧响应防护**：Astro `McpElicitationBroker` 使用 `(server,id)`、单调 token、容量上限和 `PendingCleanup`；request 监听 cancellation，`resolve_generation` 校验 token（`crates/agent-mcp/src/elicitation.rs:31-67,94-180,218-248`）。Desktop 端用 epoch/revision 快照、request key 与 inflight 防重（`apps/desktop/src/hooks/chat/usePendingInteractions.ts:11-81`）。核心语义覆盖 `3436cad5ab`。
- **异步结构化问题**：`request_user_input_async` 已是非阻塞原生工具，输出 durable questions 并进入统一消息/UI 链（`crates/agent-tools/src/builtin/hitl/request_user_input_async.rs:1-88`）。Codex 本轮主要补 TUI 编辑、队列和 feature gate，Astro 不需要按 TUI 组件照搬。
- **模型缓存身份失效**：Astro cache entry 以 provider ID 分桶，并用 provider 配置、解析后的 credential 和 custom definition 生成 fingerprint；配置/凭证变化会失效（`commands/providers/core.rs:683-776,1507-1520,2877-2899`），覆盖 Codex `f31bd3adff`/`f046cf35df` 的核心边界。
- **MCP 热刷新基础**：Astro 每个 sampling 边界执行 `reload_tools_and_mcp()`，Step 内冻结 `ToolRouter`；MCP hub 使用 generation 抑制旧 refresh。差距归入 P0 的“设置也必须随同冻结”，不是另建一套刷新机制。
- **工具事件大小控制**：Astro `bounded_tool_completed_event*` 和 tool spill 已限制 UI/模型载荷；这不等于已具备 result metadata 信任分区，因此仍保留上表 P1。

## 不建议照搬

- **Secure Enclave user verification**：Astro 已有 HITL、MCP elicitation、桌宠弹窗与明确的 session/permanent 确认，但没有企业身份签名需求。普通审批不能冒充 native identity proof；只有出现审计签名/设备绑定需求时才单独立项。
- **memory v2 dual writing**：Astro 的配置/数据迁移原则是不运行时双写或回退。Codex `2cbbf0c9b5` 的 readiness/dual-write 是其迁移策略，不能直接复制；若升级 memory schema，应采用 Astro 离线、可恢复迁移流程。
- **Windows MXC、Codex voice/TUI 动画、personality 移除、发布管道**：平台、产品形态或交互策略不同，只吸收安全与生命周期不变量，不按文件或功能名迁移。
- **完整 daemon recovery**：Astro 当前主路径是 Tauri 内嵌 backend；在没有多进程 writer 产品需求前，不应为对齐而引入 daemon 状态机。

## 建议优先级与实施顺序

1. **P0 Step settings snapshot**：先把当前已存在的 `TurnProviderSettings` 解析为 immutable step 字段，迁移 prompt/provider/tool/hook/telemetry 消费者，并用并发边界测试证明一次 step 不混代。
2. **P1 tool result metadata**：先定信任与大小契约，再接 MCP/direct tool 结果和 rollout；这是 thread attachment、Guardian evidence、审计的共同底座。
3. **P1 thread attachments**：在 metadata 边界稳定后建立持久 CRUD、分页和通知；保持与文件 artifacts 解耦。
4. **P1 worktree inventory/owner**：补 `list`、owner binding、orphan/recovery UI 与安全删除；优先服务现有 parallel task worktree，不先做永久 worktree 产品扩张。
5. **P2 credential broker**：仅随 managed proxy 的明确凭证隔离需求推进，先 threat model 和 secret-flow 测试。

上一份报告的 macOS `TIOCSTI` P0 仍未关闭：当前 `crates/agent-sandbox/src/macos.rs:104-116` 仍允许 PTY `file-ioctl`，末尾没有 `(deny file-ioctl (ioctl-command TIOCSTI))`。这不是本次 462 个提交中的新发现，但应继续单独追踪。上一份报告建议的自由文本 async tool 不应在没有新产品需求时重复推进：当前项目文档明确选择只保留结构化 `request_user_input_async`。

## 未覆盖范围 / 失败项

- 未逐行审计 2,289 个文件；先遍历全部 462 个提交标题和 Git 统计，再用两个仓库的 CodeGraph 与定向源码检查 runtime/provider/tools/MCP/security/protocol/persistence/subagent/worktree/UI 高信号链路。
- 未运行 Codex 或 Astro 测试：本任务只写报告，未修改实现；结论来自 Git diff、CodeGraph、当前源码与已有测试文件。
- 未在 Windows、Secure Enclave、voice/GStreamer 或远端 daemon 环境执行真实运行验证。
- Codex 拉取输出很大，命令完成且退出码为 0；之后已重新核验 HEAD、`FETCH_HEAD` 与工作树状态。
- Astro 是共享脏工作树；并行改动在审阅期间可能继续变化。本报告只记录检查时状态，不把这些差异纳入提交。

## 结论

本轮最值得 Astro 立即吸收的是 Codex 的完整 Step settings snapshot。Astro 已经有正确的 Step 抽象和冻结 ToolRouter，只差把 provider/model/approval/service-tier/telemetry 一并收进同一快照；这是小型类型重构但横跨关键执行链，能消除配置更新与飞行中请求/工具调用之间最难复现的一类混代问题。

随后应完成 thread attachment 的 durable contract 和 worktree inventory/owner 控制面。Credential broker 很有价值，但应以 managed proxy 的明确产品需求为前提。MCP elicitation 和模型缓存本轮无需重复建设；Windows MXC、Secure Enclave、voice/TUI 与 memory dual-write 不应机械照搬。
