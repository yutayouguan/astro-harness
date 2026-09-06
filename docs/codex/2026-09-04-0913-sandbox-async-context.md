# Codex 上游增量与 Astro 对齐报告：沙箱、异步消息与保留上下文

## 执行摘要

- 执行时间：2026-09-04 09:13 CST（Asia/Shanghai）。
- Codex 仓库：`/Users/iswm/CodeRope/codex`，分支 `main`。
- 拉取结果：安全 fast-forward，`a0dcfe2ada3f5bbd5059a34c0fc6fac244741a67..956aa3f6372ffd73a0df639beff356b3b664b858`。
- 增量规模：95 个提交、815 个文件、`+35,371/-5,526`。
- 提交时间分布（作者时间，UTC）：2026-09-02 56 个、2026-09-03 36 个、2026-09-04 3 个。
- Astro 对照基线：分支 `refactor/minimal-chat-layout`。检查开始时已有 3 个与本报告无关的未提交文件：`crates/agent-core/src/runtime/tool_router.rs`、`crates/agent-providers/src/dispatch.rs`、`crates/agent-tools/src/engine/registry.rs`；本次未修改或暂存它们。
- 核心结论：发现 1 个应立即修复的 macOS 沙箱问题、3 个高价值对齐项、2 个中优先级控制面增强；其余主要变化已对齐、仅适用于 Codex Windows/TUI/远端执行架构，或应继续观察。

## 拉取与范围证据

| 项目 | 结果 |
|---|---|
| 拉取前 HEAD | `a0dcfe2ada3f5bbd5059a34c0fc6fac244741a67` |
| 拉取后 HEAD | `956aa3f6372ffd73a0df639beff356b3b664b858` |
| Git 范围 | `a0dcfe2ada3f5bbd5059a34c0fc6fac244741a67..956aa3f6372ffd73a0df639beff356b3b664b858` |
| 提交计数 | `git rev-list --count` = 95 |
| 变更统计 | 815 files changed, 35,371 insertions, 5,526 deletions |
| 拉取后状态 | `main` 与 `origin/main` 同步，Codex 工作树干净 |

本报告只使用本地 Git 范围、提交正文和当前源码作为变更事实，不用公开 changelog 推断本次 pull。

## 按时间顺序的新增内容

### 2026-09-02：运行时耐久性、Windows 托管沙箱、异步交互与 app-server 能力

当天 56 个提交。最重要的演进依次为：Guardian 恢复/回滚覆盖和已验证答案保留（`389dd56459`、`5971d42847`、`8e3b180d49`）；目标原生 cwd 与安全审批（`eb078b4f44`）；持久化 reasoning 配置更新（`0d502a4230`）；Windows 沙箱服务的认证、策略和生命周期（`1bc8fb16ae` 至 `830363bd7c`）；自由文本异步用户消息（`d6350e24be`）；托管 worktree 枚举（`a2a9a43476`）；损坏历史记录容错与统一 JSONL 解码（`095ac4f131`、`69cebb5d15`）；MCP OAuth store 与 RMCP 3.2（`312709252d`、`a28aab7587`）；线程 environment、应用网络要求和 daemon 能力逐步进入 app-server 契约。

### 2026-09-03：MCP OAuth 协调、控制面动态发现、Guardian retained context 与安全加固

当天 36 个提交。主线包括：显式插件 mention 在 MCP 启动阶段生效（`460b63e5f4`）；协调式 OAuth refresh（`88912c04cd`）；远端 environment、permission profile、实验能力和 collaboration mode 由 app-server 动态发现（`2b554fd3f9`、`d4dc882998`、`c9fecd3fa0`、`cac96cd7b1`）；TUI steer 以 submission ID 确认（`8b8ee28a9b`）；thread originator 进入持久化/API（`728cb12fe5`）；MCP auth challenge 和工具发现失败不再丢失（`0650d6d1ca`、`8f31b64c7f`）；Guardian 将用户已验证答案持久化到线程保留上下文并校验压缩 checkpoint（`1d74c3ba1e`、`ad8ee16a5f`）；macOS Seatbelt 明确禁止 `TIOCSTI` 终端输入注入（`ec84e69261`）；新增远端 exec trusted headers、GPT-6-Astra catalog、Noise 握手超时、图像 detail 统一和可注入附件存储（`801ca0d0d1`、`ed391d4dd2`、`781c183c3b`、`280ae8b9fc`、`03467026f2`）。

### 2026-09-04：模型默认值提示与 TUI 稳定性

当天 3 个提交：保存的模型默认值被覆盖时提示（`68e9c4a31a`）；加强 assistant markup 解析（`0305dde920`）；全屏 overlay 返回后恢复 inline TUI（`956aa3f637`）。这些均偏 Codex TUI，不直接映射到 Astro React/Tauri UI。

## 关键 Codex 源码证据

| 主题 | 提交与源码证据 | 语义 |
|---|---|---|
| macOS 终端输入注入 | `ec84e69261`；`codex-rs/cli/src/debug_sandbox.rs:400` | 在所有共享 Seatbelt allow 之后追加 `(deny file-ioctl (ioctl-command TIOCSTI))`，并用真实 PTY 集成测试证明未沙箱时可注入、沙箱时返回 `EPERM`。 |
| 自由文本异步消息 | `d6350e24be`；`codex-rs/core/src/tools/handlers/send_message_to_user_async.rs:21-100`；`spec_plan.rs:1184-1195` | 独立于结构化 `request_user_input_async`，立即发出 `AgentMessageDelivery::Async`，不结束 turn；仅 root agent 且 model catalog 显式 opt-in。 |
| Guardian 保留上下文 | `5971d42847`、`8e3b180d49`、`1d74c3ba1e`、`ad8ee16a5f`；`codex-rs/history/src/retained_context.rs:10-129` | 主机验证的问答有数量/字节上限，随 checkpoint 持久化；压缩不删除，rollback 按来源裁剪；证据缺失时 fail closed。 |
| OAuth refresh 协调 | `312709252d`、`88912c04cd`；`codex-rs/rmcp-client/src/oauth/credential_store.rs:46-159`、`oauth/runtime.rs:20-63` | 同一 credential identity 的 refresh 由事务 guard 串行化；锁内重新读取 Keychain 并检查 freshness/issuer，取消调用也不会中断持久化。 |
| 托管 worktree 枚举 | `a2a9a43476`；`codex-rs/worktree/src/lib.rs` 的 `WorktreeManager::list` | 通过 `git worktree list --porcelain -z` 枚举，并校验 managed layout、common-dir、backlink、别名、stale registration 和安全 cwd。 |
| 线程环境/来源 | `2b554fd3f9`、`728cb12fe5`；`app-server-protocol/src/protocol/v2/environment.rs:9-31`、`thread_data.rs:263-277` | thread API 同时暴露 cwd、runtime roots、environment ID、originator 和 session source，支持本地/远端控制面正确恢复和过滤。 |
| 附件存储抽象 | `03467026f2`；`codex-rs/attachment-store/src/lib.rs:19-77`、`core/src/thread_manager.rs:360,421,446,686` | `AttachmentStore` 将字节持久化与 durable ref 解耦；默认仍是 data URL，但 ThreadManager 可注入远端/持久实现。 |

## Astro 对齐矩阵

| 优先级 | 结论 | 对齐项 | Astro 当前证据 | 建议 |
|---|---|---|---|---|
| P0 | 建议立即对齐 | macOS Seatbelt 禁止 `TIOCSTI` | `crates/agent-sandbox/src/macos.rs:104-116` 对 PTY 设备允许 `file-ioctl`，生成 profile 末尾没有更具体的 `TIOCSTI` deny。 | 在 profile 最末追加精确 deny，并增加真实 PTY/Seatbelt macOS 测试；覆盖 `terminal`、`exec_command`、`code_exec` 共享入口。 |
| P1 | 建议对齐 | 独立的 `send_message_to_user_async` | Astro 已具备完整承载链：`AgentMessageDelivery::Async`、`emit_async_agent_message`（`crates/agent-core/src/streaming/lifecycle.rs:437-468`）、Desktop `upsertAsyncAgentUpdate`；但模型工具只有要求非空 questions 的 `request_user_input_async`（`crates/agent-tools/src/builtin/hitl/request_user_input_async.rs:12-138`）。 | 新增语义独立、root-only、catalog-gated 的自由文本工具，直接复用已有 protocol/rollout/Tauri/React 路径；不要把它做成旧名称兼容别名。 |
| P1 | 建议分阶段对齐 | Guardian verified answers retained context | Astro `GuardianRetryState` 明确只是内存 one-shot bridge（`crates/agent-core/src/control/guardian.rs:17-78`）；压缩用 replacement history，rollback 直接重建并替换历史（`crates/agent-core/src/runtime/history_control.rs:72-133`、`crates/agent-rollout/src/reconstruction.rs:63-134`），没有独立、限界、主机验证的问答事实层。 | 在 `agent-protocol`/`agent-rollout` 定义 bounded retained event + checkpoint；只接受 host-confirmed 答案，压缩保留、rollback 按来源删除、恢复重放；Guardian 不完整时 fail closed。 |
| P1 | 建议对齐 | MCP OAuth refresh 单飞和取消安全 | Astro 已把 registration/token 放入 Keychain，并用 issuer/state 校验；但 `http_client_for` 每次新建 `AuthorizationManager`，`McpKeyringCredentialStore` 直接 load/save/clear（`crates/agent-mcp/src/auth.rs:118-254`），没有跨连接 refresh transaction。 | 按 server ID + URL/issuer 建立共享 refresh guard；锁内重读凭证和 freshness，刷新任务独立于请求取消；补并发 refresh、替换登录和取消测试。 |
| P2 | 建议对齐 | `WorktreeManager::list` 与恢复审计 | Astro `WorktreeManager` 只有 `create`（`crates/agent-delegate/src/lib.rs:53-140`）和 cleanup 入口，没有可信枚举 API。 | 增加严格过滤的 `list`，再通过 Tauri/Desktop 展示和清理孤儿 worktree；复用现有不信任 symlink、common-dir/backlink 的安全原则。 |
| P2 | 部分对齐 | 结构化 thread environment/originator | Astro 持久化 `source` 与 `project_id`，且 turn 内有 project/workspace roots；但 `RecentSessionDto` 只暴露 source/project/title/time（`apps/desktop/src-tauri/src/commands/session.rs:13-24`），没有结构化 environment/root/originator。 | 在远端 exec/worktree 恢复成为产品能力前，先统一共享 DTO 与持久字段；避免只在 live runtime 中持有 roots。 |
| P3 | 观察后再对齐 | 可注入 `AttachmentStore` | Astro 已有 `agent-artifacts`、session/message 关联和本地附件 UI，但 `ThreadManager` 只管理 runtime/listener（`crates/agent-server/src/thread_manager.rs:9-16,96-100`），附件仍由 Tauri payload/local path 路径处理。 | 等远端附件、跨设备恢复或内容寻址成为明确需求后，把现有 artifacts DB 实现成 trait 后端；现在不应为 Codex 的前置抽象重构整条链。 |

## 已对齐或不建议照搬

### 已对齐

- **持久 reasoning/service tier 更新**：Astro 已有 `UpdateTurnSettingsRequest`、`ThreadSettingsAppliedEvent`、rollout 持久策略、冷恢复投影和下一 step 原子应用；无需重复实现 `0d502a4230`。
- **steer 的提交身份确认**：Astro 的 `client_message_id`、`UserInputCommittedEvent` 和先持久化再 ACK 路径已经覆盖 `8b8ee28a9b` 的核心不变量。
- **损坏 JSONL 容错**：`agent-rollout::read_rollout_with_diagnostics` 逐行跳过解析错误并累计 `parse_errors`，统一由 `effective_response_history` 应用 compact/rollback；基本覆盖 `095ac4f131` 和 `69cebb5d15`。
- **MCP 启动/发现错误可见性**：Astro 只有在 `list_all_tools` 成功后才建立 `RunningServer`；失败进入 `last_connect_errors`，并通过 `ServerStatus.error` → gRPC → Tauri → React 展示。其状态机不会把“发现失败”误表示为“成功的空 catalog”，因此不必机械增加 Codex 的 `toolsError` 字段。

### 暂不建议照搬

- **Windows sandbox service/daemon 系列**：占本轮较大体积，但 Astro 当前本地沙箱和内嵌 server 架构不同。除非近期明确交付 Windows 托管沙箱，不应让这一系列提交挤占 macOS P0。
- **Codex TUI 拆分、overlay、Vim/markup 修复**：Astro 是 React/Tauri 界面，不能按文件或组件名称迁移；只在出现等价交互缺陷时吸收行为原则。
- **GPT-6-Astra catalog**：先通过 Astro 的 provider/catalog 发布流程确认模型可用性、后端能力和计费元数据，不因上游出现名称就直接写入生产目录。
- **独立 `toolsError`**：Astro 当前连接生命周期已具备无歧义失败语义；只有未来允许“连接成功但 catalog 获取失败并继续运行”时才需要拆字段。

## 推荐实施顺序

1. **P0 沙箱补丁**：为共享 Seatbelt profile 添加末尾 `TIOCSTI` deny，并写 macOS PTY 回归测试。这是安全边界修复，改动小、收益确定。
2. **P1 异步自由文本工具**：新增模型工具和 catalog/root-agent gate，复用现有 async item 全链路；测试“发出后 turn 继续”和 subagent 不暴露。
3. **P1 MCP OAuth 单飞**：先建立 credential identity 与事务锁，再接取消安全持久化；避免多个 agent/session 同时刷新导致 token 覆盖。
4. **P1 Guardian retained context**：拆成共享类型/rollout checkpoint、runtime restore/rollback、Guardian rendering 三个独立批次，每批都要有重启和 compaction 集成测试。
5. **P2 worktree list + thread provenance**：作为 Desktop 控制面恢复能力推进，不与前四项耦合。

## 未覆盖范围与验证边界

- 本次是源码审阅和报告，不修改 Astro 实现，因此未运行 Rust/TypeScript 测试。
- 未逐行审计 815 个文件；通过完整提交清单、目录统计、关键 commit diff 和 Astro 当前源码链路覆盖高信号区域。
- 未验证 Codex 的 Windows sandbox service、远端 exec/Noise、GStreamer/voice 打包在真实目标平台上的运行行为。
- Astro 对照包含当前工作树的 3 个既有未提交文件；本报告没有覆盖或暂存这些变更。
- 模型 catalog 条目只作为本地源码变化记录，不等同于外部服务当前可用性声明。

## 结论

本轮最值得立即吸收的是 `ec84e69261` 的 macOS `TIOCSTI` 防护。Astro 当前主动允许 PTY `file-ioctl`，却没有末尾精确 deny，存在与 Codex 修复原因相同的终端输入注入面。

随后应利用 Astro 已完成的异步消息协议/UI 基础，低成本增加独立的自由文本 `send_message_to_user_async`。Guardian retained context 和 MCP OAuth refresh coordination 都是耐久性/并发正确性提升，但应拆批落地。Worktree 枚举与结构化 thread provenance 是控制面完善项；附件 store 目前继续观察更合适。

## 完整提交清单（作者时间，UTC，正序）

- `8d32abcd01` — `2026-09-02T11:56:44Z` — Report the exec-server release version in environment info (#42270)
- `50fffd5ed3` — `2026-09-02T14:03:09Z` — Refresh plugin skills after out-of-process version changes (#42284)
- `94e5d05095` — `2026-09-02T14:29:17Z` — Fetch rules_rs zlib packages from Ubuntu snapshots (#42288)
- `389dd56459` — `2026-09-02T14:45:41Z` — Expand Guardian history coverage across resume and rollback (#42290)
- `5971d42847` — `2026-09-02T14:48:52Z` — Preserve verified answers across history compaction (#42293)
- `8e3b180d49` — `2026-09-02T15:35:55Z` — Preserve retained answers across steer rollbacks (#42298)
- `fc953e5234` — `2026-09-02T16:26:26Z` — Stabilize the detached exec-server session resume test (#42306)
- `1bc8fb16ae` — `2026-09-02T16:38:52Z` — Separate Windows sandbox provisioning from ACL refresh (#42309)
- `eb078b4f44` — `2026-09-02T17:16:40Z` — Preserve target-native cwd in permission approval requests (#42314)
- `f252c23b88` — `2026-09-02T17:31:20Z` — Refactor exec-server startup futures (#42316)
- `a94a5db629` — `2026-09-02T17:36:45Z` — Support packaged managed Codex binary paths (#42318)
- `a526f54b00` — `2026-09-02T17:37:50Z` — Show live context compaction status in the TUI (#42319)
- `5e26f7621c` — `2026-09-02T17:38:23Z` — Make the app-server thread unload delay configurable (#42320)
- `637c3227b3` — `2026-09-02T17:50:51Z` — Avoid executing PATH helpers before workspace trust (#42324)
- `e6ff749506` — `2026-09-02T18:20:33Z` — Render completed assistant messages directly during replay (#42325)
- `73e94ee7a6` — `2026-09-02T18:20:36Z` — Harden Windows control socket rendezvous (#42326)
- `0d502a4230` — `2026-09-02T18:30:52Z` — Support durable reasoning configuration updates (#42328)
- `f59905647a` — `2026-09-02T18:38:08Z` — Protect Windows sandbox binaries from inherited write access (#42330)
- `dc0dc4f15d` — `2026-09-02T18:38:37Z` — Package prepared runtimes with the voice host (#42332)
- `301a7c5e01` — `2026-09-02T18:40:46Z` — Add a Windows sandbox provisioning protocol (#42334)
- `dcfcb570b2` — `2026-09-02T18:40:46Z` — Add an authenticated Windows sandbox provisioning client (#42337)
- `501931b399` — `2026-09-02T18:40:47Z` — Add Windows sandbox service lifecycle scaffolding (#42341)
- `add870a4bf` — `2026-09-02T18:40:47Z` — Harden Windows sandbox provisioning file handling (#42342)
- `c4ea7294b9` — `2026-09-02T18:40:48Z` — Prepare managed policy validation for Windows sandbox provisioning (#42344)
- `4fdf4c1113` — `2026-09-02T18:40:48Z` — Add Windows sandbox client authentication (#42348)
- `7e45bdb5fd` — `2026-09-02T18:40:49Z` — Enable authenticated Windows sandbox provisioning (#42351)
- `830363bd7c` — `2026-09-02T18:40:49Z` — Add experimental Windows sandbox service provisioning (#42353)
- `d6350e24be` — `2026-09-02T18:44:29Z` — Add free-form asynchronous user messages (#42354)
- `0227158fd5` — `2026-09-02T18:47:42Z` — Initialize questions in buffered replay test messages (#42356)
- `577a4fcd06` — `2026-09-02T19:54:04Z` — Extend rate limit reads with usage capabilities (#42358)
- `10aca93f18` — `2026-09-02T19:55:03Z` — Support graceful daemon shutdown on Windows (#42364)
- `a2a9a43476` — `2026-09-02T20:14:04Z` — List managed worktrees for a repository (#42366)
- `095ac4f131` — `2026-09-02T20:18:50Z` — Keep SQLite history projection moving past invalid records (#42369)
- `76f47103fe` — `2026-09-02T20:20:20Z` — Improve MCP server startup error logging (#42370)
- `5037919777` — `2026-09-02T20:25:48Z` — Add Luna Reserve usage fallback to the TUI (#42372)
- `f53c91be2c` — `2026-09-02T20:32:12Z` — Add attributed exec process lifecycle telemetry (#42373)
- `a14ef02e1c` — `2026-09-02T20:38:53Z` — Extract PID startup into a dedicated module (#42374)
- `665e5f45ab` — `2026-09-02T20:54:17Z` — Clean up Windows sandbox resources on app uninstall (#42375)
- `e1d0ef995f` — `2026-09-02T21:04:25Z` — Make app-server realtime sessions always available (#42377)
- `69cebb5d15` — `2026-09-02T21:21:54Z` — Route rollout reads through the canonical JSON decoder (#42378)
- `0588fc941c` — `2026-09-02T21:34:01Z` — Require confirmation for safety-buffered retries (#42380)
- `715294448f` — `2026-09-02T21:44:50Z` — Support managed app-server lifecycle on Windows (#42381)
- `a28aab7587` — `2026-09-02T21:59:24Z` — Update rmcp to 3.2.0 (#42383)
- `312709252d` — `2026-09-02T22:07:23Z` — Add an RMCP OAuth credential store adapter (#42384)
- `cff76fa96f` — `2026-09-02T22:13:39Z` — Add experimental context management activation (#42385)
- `2b554fd3f9` — `2026-09-02T22:31:43Z` — Expose loaded thread environments in app-server responses (#42386)
- `e6249b5296` — `2026-09-02T22:32:20Z` — Recover deferred environments after provisioning failure (#42388)
- `fe140d4c8e` — `2026-09-02T22:33:13Z` — Authorize `apply_patch` in the executor path context (#42391)
- `91608236ea` — `2026-09-02T22:44:23Z` — Support managed daemon updates on Windows (#42392)
- `9bb1ea035f` — `2026-09-02T22:46:56Z` — Expose the Codex version to commands and turn metadata (#42395)
- `e6a944ad75` — `2026-09-02T23:02:50Z` — Extract focused TUI logic into submodules (#42397)
- `54a4077c8b` — `2026-09-02T23:13:36Z` — Preserve restored input after resolved misalignment errors (#42399)
- `d4dc882998` — `2026-09-02T23:14:26Z` — Discover TUI collaboration modes from the app server (#42401)
- `1281778e32` — `2026-09-02T23:29:24Z` — Expose the last accepted environment ready report (#42403)
- `deb1471166` — `2026-09-02T23:42:48Z` — Read voice helper frames independently of pipe chunks (#42404)
- `b7f710273e` — `2026-09-02T23:45:43Z` — Support the app-server daemon on Windows (#42405)
- `460b63e5f4` — `2026-09-03T00:07:26Z` — Honor explicit plugin mentions during MCP startup (#42406)
- `93053c7f5d` — `2026-09-03T00:10:29Z` — Harden embedded composer input handling (#42408)
- `fdf23b4097` — `2026-09-03T00:28:59Z` — Allow reviewing and continuing misalignment-paused chats (#42410)
- `88912c04cd` — `2026-09-03T00:33:45Z` — Enable coordinated MCP OAuth refresh (#42413)
- `b27a6321fa` — `2026-09-03T00:34:10Z` — Expose managed application network requirements (#42417)
- `1d741742c5` — `2026-09-03T00:39:05Z` — Add session resume to the agent command center (#42419)
- `38ba8cdceb` — `2026-09-03T00:40:31Z` — Honor model requirements in Guardian computer-use scoring (#42422)
- `cac96cd7b1` — `2026-09-03T02:14:29Z` — Discover TUI experimental features from the server (#42425)
- `62f553bfd0` — `2026-09-03T03:04:23Z` — Use the shared composer in the agent command center (#42428)
- `498d40b29f` — `2026-09-03T03:06:17Z` — Box the TUI resume picker future (#42432)
- `36984da442` — `2026-09-03T04:26:12Z` — Include originator in plugin measurement analytics (#42445)
- `8b8ee28a9b` — `2026-09-03T05:05:00Z` — Acknowledge pending TUI steers by submission ID (#42451)
- `c9fecd3fa0` — `2026-09-03T05:05:29Z` — Discover permission profiles from the app server (#42453)
- `8ff74cc9b1` — `2026-09-03T05:10:00Z` — Show live task details in the agent command center (#42455)
- `728cb12fe5` — `2026-09-03T05:52:26Z` — Expose thread originators through the app-server API (#42458)
- `6d7f6dcd22` — `2026-09-03T14:24:03Z` — Register the Guardian thread context feature flag (#42529)
- `0650d6d1ca` — `2026-09-03T15:00:46Z` — Preserve MCP authentication challenges on tool calls (#42552)
- `7a7c188682` — `2026-09-03T16:10:18Z` — Preserve target-native paths in command approvals (#42577)
- `1d74c3ba1e` — `2026-09-03T16:31:11Z` — Persist verified user answers in Guardian thread context (#42579)
- `1d6727c0b5` — `2026-09-03T17:09:08Z` — Recover Vim escape input in legacy terminals (#42584)
- `ad8ee16a5f` — `2026-09-03T17:24:52Z` — Require Guardian review for incompatible compaction checkpoints (#42588)
- `ec84e69261` — `2026-09-03T17:51:48Z` — Harden the macOS sandbox against terminal input injection (#42590)
- `2387310b52` — `2026-09-03T18:03:53Z` — Reload user config after local plugin installation (#42593)
- `f60dfe80b5` — `2026-09-03T18:06:23Z` — Record Windows sandbox private desktop usage (#42596)
- `8f31b64c7f` — `2026-09-03T18:23:17Z` — Report MCP tool discovery errors in server status (#42598)
- `f84c9776dc` — `2026-09-03T19:20:18Z` — Deprecate detached review delivery (#42602)
- `7eee24ef51` — `2026-09-03T19:33:56Z` — Expose global metrics installation in `codex-otel` (#42603)
- `801ca0d0d1` — `2026-09-03T19:34:18Z` — Support trusted headers for remote exec WebSockets (#42606)
- `ed391d4dd2` — `2026-09-03T19:47:07Z` — Add GPT-6-Astra to the bundled model catalog (#42607)
- `32c303c197` — `2026-09-03T20:16:14Z` — Condense TUI startup warnings (#42609)
- `1f7b99922a` — `2026-09-03T21:10:52Z` — Add GPT-6-Astra to Amazon Bedrock catalogs (#42619)
- `781c183c3b` — `2026-09-03T21:22:00Z` — Bound Noise handshakes by the exec server initialization timeout (#42623)
- `280ae8b9fc` — `2026-09-03T22:00:07Z` — Centralize prompt image detail modes (#42624)
- `d979df154c` — `2026-09-03T22:41:26Z` — Initialize the packaged GStreamer runtime in the voice host (#42631)
- `03467026f2` — `2026-09-03T23:10:59Z` — Add an injectable attachment store to ThreadManager (#42634)
- `0e0f55fc4e` — `2026-09-03T23:54:50Z` — Update GPT-6-Astra Fast tier speed description (#42638)
- `68e9c4a31a` — `2026-09-04T00:00:18Z` — Warn when saved model defaults are overridden (#42639)
- `0305dde920` — `2026-09-04T00:00:28Z` — Harden TUI parsing of assistant markup (#42640)
- `956aa3f637` — `2026-09-04T00:15:35Z` — Restore the inline TUI after full-screen overlays (#42641)
