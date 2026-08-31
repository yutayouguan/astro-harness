# Astro 代码缺陷交接

> 规则：在 Agent/Harness 对齐过程中发现代码缺陷时，先记录证据、影响、复现和建议，不直接修改实现。
>
> 更新时间：2026-08-30

## 状态约定

- `Open`：证据充分，尚未修复。
- `NeedsValidation`：存在风险信号，需要补充复现或测试。
- `FixedPendingVerification`：已有修复，尚未完成回归。
- `Closed`：修复和回归均完成。

## H-001 数据库迁移目标与默认打开路径不一致

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | High |
| 发现日期 | 2026-08-29 |
| 发现阶段 | Agent Harness 文档与源码对齐 |
| 是否已修复 | 是；已完成实现、回归与文档收敛 |

### 问题描述（修复前）

`agent-home::ensure_workspace_dirs()` 把多个旧数据库迁移到 `{base}/data/`，但当前部分默认 helper 和生产调用仍从旧路径打开数据库。迁移一旦发生，后续旧路径调用可能创建新的空数据库；与此同时，部分新调用已经使用 `{base}/data/`，从而形成同一进程或不同功能读取不同数据库的 split-brain。

### 源码证据（修复前）

迁移目标：

- `crates/agent-home/src/workspace/paths.rs:295-317`
  - `{base}/usage.db` → `{base}/data/usage.db`
  - `{base}/subagents-v2.db` → `{base}/data/subagents-v2.db`
  - `{base}/sessions/state.db` → `{base}/data/state.db`
  - artifacts、knowledge、cron 数据库也采用相同迁移方向。

仍使用旧路径的入口：

- `crates/agent-usage/src/db.rs:158-160`：`usage_db_path()` 返回 `{base}/usage.db`。
- `crates/agent-subagents/src/store.rs:36-38`：`v2_default_db_path()` 返回 `{base}/subagents-v2.db`。
- `crates/agent-server/src/grpc/astro_service.rs:39-44`：SessionStore 从 `{base}/sessions/state.db` 打开。
- `crates/agent-memory/src/agent/workspace/lifecycle.rs:20-28`：workspace 初始化仍创建 `{base}/sessions/state.db`。

已经使用新路径的入口：

- `crates/agent-tools/src/engine/context.rs`、`dispatch.rs` 以及 shell/media/HITL 工具中的多个调用从 `{base}/data/state.db` 打开 SessionStore。

迁移触发入口：

- `crates/agent-usage/src/stats.rs`
- `crates/agent-home/src/config/tools_enabled.rs`
- `crates/agent-cron/src/jobs/store.rs`
- `crates/agent-mcp/src/config.rs`

这些模块会调用 `ensure_default_workspace_dirs()`，因此问题不局限于显式数据迁移命令。

### 可能影响

1. 迁移后会话历史在桌面端或 Server 中看似消失，因为旧路径被重新创建为空 `state.db`。
2. 工具执行与主对话可能分别读写 `data/state.db` 和 `sessions/state.db`。
3. usage 和 Agent Graph 可能在新旧位置产生两份数据库，造成统计、mailbox 或恢复状态不一致。
4. 如果数据库正处于 WAL 模式，直接移动主文件而未协调连接与 sidecar 文件还需单独评估。

### 建议复现测试

只在临时 `ASTRO_MEMORY_DIR` 中执行：

1. 在旧路径创建带唯一记录的 `usage.db`、`subagents-v2.db` 和 `sessions/state.db`。
2. 调用 `ensure_workspace_dirs()`。
3. 分别调用 Usage、AgentGraph 和 Session 的默认生产入口。
4. 断言打开路径均为迁移后的同一 canonical 位置。
5. 断言唯一记录仍可见，且旧路径未生成第二份空库。
6. 增加 WAL/SHM 存在时的迁移测试，并验证进程重启后的恢复行为。

### 已采用的修复方向

- `agent-home` 集中定义 Session、usage、Agent Graph、artifacts、knowledge 和 Cron 运行库 resolver，canonical 目录统一为 `{base}/data/`。
- Usage、AgentGraph、Artifact、Knowledge 和 Cron 的默认入口先执行 workspace 迁移，Server、Desktop、Agent runtime 和辅助任务调用不再打开旧 Session 路径。
- SQLite 迁移将主库、`-wal`、`-shm` 作为文件族处理；单个文件族中途失败时回滚已移动成员。
- 旧库和 canonical 主库同时存在时直接报错，不覆盖任何一份数据；旧 `cron/cron.db` 迁移时同时重命名为 `data/cron_v1.db`。

### 验证结果

- `cargo check -p agent -p server -p subagents -p astro-agent`：通过。
- `cargo test -p home`：通过，包含 canonical resolver、SQLite 文件族迁移、split-brain 拒绝和旧 Cron 文件名迁移。
- `cargo test -p usage -p artifacts -p cron -p memory --lib`：通过。
- `cargo test -p usage --test usage_db_test migrated_legacy_usage_database_remains_visible -- --exact`：通过，确认迁移后 usage 记录可见且旧库不会重建。
- `cargo test -p agent --test cron_exec_test`：通过。
- `cargo test -p session --lib`：通过；`dispatch_session_tool_test` 的 2 个用例单独执行通过。
- `memory::ensure_workspace_migrates_legacy_session_without_creating_a_second_database` 验证迁移后原 Session 数据可见，且不重建旧库。

### 验收标准

- 同一职责数据库只有一个 canonical 路径。
- 所有默认 helper、Server、Desktop、Cron、工具和测试使用同一个 resolver。
- 旧路径迁移幂等，已有目标文件时有明确冲突策略。
- WAL/SHM、失败回滚和跨版本恢复均有覆盖。
- 迁移前后的会话、usage、Agent Graph 数据可见性一致。

## H-002 Agent Core test-support 未跟随异步存储 API

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Medium |
| 发现日期 | 2026-08-29 |
| 发现阶段 | H-001 相关 crate 回归测试 |
| 是否已修复 | 是；已由 `37b254b9` 完成异步化，本轮复核并关闭记录 |

### 问题描述

`crates/agent-core/src/exec/dispatch/test_support.rs` 的多个同步辅助方法仍直接对已经返回 `Future` 的 `AgentGraphStore`、`AgentControl` 和 `SessionStore` API 使用 `?`。当其他 crate 的测试通过 dev-dependency 启用 Agent test-support 时，`agent` 无法编译。

### 验证证据

执行：

```bash
CARGO_TARGET_DIR=/tmp/astro-harness-doc-check \
  cargo test -p home -p usage -p artifacts -p cron -p memory -p subagents
```

编译器在 `dispatch/test_support.rs` 的 `LifecycleTestApp::new`、`restart_with`、`dispatch_at`、`status`、`pending_mailbox`、`session_contents` 和 `session_ids` 等位置报告 `E0277`，提示应对返回的 Future 使用 `.await`，但所在方法当前不是 async。

### 建议处理

单独设计 test-support 的异步边界：将辅助 API 整体异步化，或通过明确的测试 runtime 适配；不要在零散调用点使用阻塞嵌套 runtime。修复后恢复 `subagents` 及依赖 `agent/test-support` 的测试覆盖。

### 已采用的修复

- `LifecycleTestApp::new`、`restart_with`、`dispatch_at`、`status`、`pending_mailbox`、`session_contents`、`session_ids` 和依赖方法统一改为 async。
- `AgentGraphStore`、`AgentControl`、`SessionStore` 调用在原 Tokio runtime 中直接 `.await`，没有引入阻塞式嵌套 runtime。
- `crates/agent-subagents/tests/v2_lifecycle.rs` 的所有调用点同步改为 async 调用。

### 验证结果

- `cargo test -p subagents --test v2_lifecycle -- --nocapture`：1 项通过。
- `cargo test -p subagents --tests`：101 项通过。

## H-003 Legacy Provider 的 Namespace 工具名不符合 Function 协议

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | High |
| 发现日期 | 2026-08-29 |
| 发现阶段 | DeepSeek Chat Completions 工具请求 |
| 是否已修复 | 是；已完成协议修复、路由兼容、回归与文档更新 |

### 问题描述

DeepSeek 返回 HTTP 400：`tools[28].function.name` 不匹配 `^[a-zA-Z0-9\_-]+$`。Astro 会将 Responses API 的 Namespace 工具降级为 Chat Completions Function，但当前将名字展平为 `{namespace}.{child}`；其中的 `.` 不是 Function name 允许字符。当前内置 Cron 工具使用 Namespace，且 Namespace 被排在普通工具之后，与报错的第 29 个工具位置一致。

### 源码证据

- `crates/agent-tools/src/engine/registry.rs`：`api_specs()` 将非空 `namespace` 组装为 Responses API Namespace。
- `crates/agent-providers/src/types/message.rs`：`ToolDefinition::function_definitions()` 将 Namespace 子工具降级为 `{namespace}.{child}` Function。
- `crates/agent-providers/src/compat/completion.rs`：Chat Completions 请求直接把上述名称写入 `tools[].function.name`。
- `crates/agent-core/src/runtime/tool_router.rs`：执行路由目前只登记 `{namespace}.{child}` wire name，因此不能只改下发名而不同步增加回程别名。

### 建议修复方向

保留 Responses API 原生 Namespace 的点分层语义；仅在降级为 Function 的 Provider 边界将名字编码为合法的 `{namespace}__{child}`，并让 step-scoped `ToolRouter` 同时接受原生 `{namespace}.{child}` 与降级 `{namespace}__{child}`，两者均映射回同一 registered handler。不应对所有工具名做无损信息的泛化字符替换。

### 已采用的修复

- `ToolDefinition::function_definitions()` 只在 Function-only Provider 降级边界将 Namespace 子工具编码为 `{namespace}__{child}`；Responses API 原生 Namespace 序列化不变。
- Chat Completions 请求体统一复用该转换，覆盖 Namespace Function 与 Namespace Freeform。
- step-scoped `ToolRouter` 同时登记 `{namespace}.{child}` 和 `{namespace}__{child}`，两者仅映射到本轮规格中已经存在的同一 registered handler。
- 设计文档和术语规范明确区分原生 wire name 与 Legacy Function wire name。

### 验证结果

- `cargo test -p providers namespace_ -- --nocapture`：通过，2 个 Namespace 降级与请求体测试通过。
- `cargo check -p providers -p agent --lib`：通过，确认 Provider 与 Agent production 路由代码可编译。
- 三个本轮 Rust 文件的 `rustfmt --check`：通过。
- `git diff --check`：通过。
- `cargo test -p agent namespace_wire_name_resolves_to_registered_handler_name`：被 H-002 的既有 test-support/异步 API 编译错误阻塞；该命令在进入目标测试前产生 238 个无关编译错误。本轮新增双 wire name 断言已写入测试源，production 编译已通过。

### 验收标准

- 降级后所有 Function name 均匹配 `^[a-zA-Z0-9\_-]+$`。
- Responses API 的原生 Namespace JSON 保持不变。
- 模型返回 `{namespace}__{child}` 时能执行原 registered handler，不扩大可调用工具集。
- DeepSeek/OpenAI-compatible Chat Completions 的请求体回归测试不再包含点分隔 Function name。

## H-004 Provider 真实用量与上下文占用脱节

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | High |
| 发现日期 | 2026-08-30 |
| 发现阶段 | Agent Harness usage/context 对齐 |
| 是否已修复 | 是；Provider、事件、回放、上下文与桌面展示已贯通 |

### 问题描述

Astro 已能从部分 Provider 读取 input、output、cache read/write 和 reasoning 用量，但目前会丢弃 Provider 原始 `total_tokens`，且用数值 `0` 同时表示“Provider 明确上报为零”和“Provider 没有上报该字段”。运行时会将累计 usage 写入事件，但桌面端只保留 prompt/completion/total 三项；上下文占用则另外使用本地字符估算，不会用 Provider 最近一次采样的实际 usage 校准。

### 源码证据

- `crates/agent-providers/src/compat/mod.rs`：`parse_openai_usage()` 读取 prompt/output 明细，但不保留 Provider 返回的 `total_tokens`。
- `crates/agent-providers/src/types/stream.rs`：`Usage::total_tokens()` 始终由本地分项重算；所有可选明细都是非可选整数，无法区分未上报与零值。
- `crates/agent-protocol/src/event.rs`：`TokenCountEvent` 包含 cache read/write 和 reasoning，但未传递原始 total 的报告状态。
- `apps/desktop/src/hooks/chat/useSend.ts`：`usage` 事件仅映射 prompt/completion/total，丢弃 cache read/write、reasoning 和 request count。
- `crates/agent-core/src/streaming/maintenance.rs`：每次采样前的 `context_usage` 仅由本地 prompt/history/tool schema 估算构建。
- `apps/desktop/src/components/chat/ContextUsagePopover.tsx`：占用量恒以 `~` 标识为估算，不显示数据来源、缓存命中或 reasoning 明细。

### 用户影响

1. 用户无法判断当前上下文环是 Provider 实际计数还是本地估算。
2. 对于 Responses API，缓存命中和 reasoning 已产生成本，但 UI 不可见。
3. 非 OpenAI Provider 或兼容层字段缺失时，会被误解为“明确为零”。
4. 如果 Provider 原始 total 与 Astro 分项重算不一致，当前没有审计信号，也无法在回放时复原。

### 建议修复方向

- Provider 层保留归一化分项、Provider 原始 total 以及可选明细的“是否上报”状态；`reasoning_tokens` 明确为 `output_tokens` 的子集，不重复计入 total。
- 累计用量继续用于计费/单轮统计；另保留最近一次 Provider usage，专用于上下文占用校准。
- 上下文快照采用混合模式：Provider actual 可用时作为顶层占用真值，本地 estimate 保留分类解释；实际数缺失时明确降级为 estimate。
- UI 展示 input、cached input、cache write、output、reasoning output、total、cache hit rate 和数据来源；缓存不作为新的上下文 segment 重复堆叠。

### 验收标准

- Responses API 的五类 usage 字段全部传递，Provider 原始 `total_tokens` 可审计。
- 缓存/reasoning 未上报与明确为零可区分。
- 上下文环优先显示最近 Provider actual，并保留本地分段估算；降级状态可见。
- `tool_search`/MCP 激活后，新暴露的工具 schema 从下一次 sampling 快照起进入本地估算。
- rollout/history 回放能复原该轮实际/估算来源与用量明细。

### 验证结果

- `cargo test -p providers --lib -p agent-protocol -p agent-rollout`：271 项通过。
- `cargo test -p agent --test streaming_test billing_token_count_total_includes_cached_tokens -- --nocapture`：通过。
- `cargo check -p agent -p server -p astro-agent`：通过。
- `node --test src/lib/chat/contextUsage.test.ts`：8 项通过。

## H-005 Context usage 分类测试与实现数量脱节

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Low |
| 发现日期 | 2026-08-30 |
| 发现阶段 | Context usage 前端回归 |
| 是否已修复 | 是；改为断言完整的 11 类稳定集合 |

### 问题与证据

`apps/desktop/src/lib/chat/contextUsage.ts` 的 `SEGMENT_ORDER` 已包含 11 类，但 `contextUsage.test.ts` 仍固定断言长度为 9。执行 `node --test src/lib/chat/contextUsage.test.ts` 时稳定失败：`11 !== 9`。

### 影响与修复方向

该失配会遮蔽 context usage 真实回归结果。测试应断言完整的稳定分类集合，而不只检查一个已过期的数量。

### 验收标准

- 聚焦测试通过。
- 缺失、重复或顺序意外变更仍能被测试检出。

### 验证结果

`node --test src/lib/chat/contextUsage.test.ts`：8 项通过。

## H-006 Desktop compaction 测试未跟随异步 Session API

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Low |
| 发现日期 | 2026-08-30 |
| 发现阶段 | Tauri usage 事件投影回归 |
| 是否已修复 | 是；两个用例已切换为 Tokio async test 并 await Session 构造 |

### 问题与证据

`apps/desktop/src-tauri/src/commands/compaction.rs` 的两个测试仍对已异步化的 `agent::Session::new()` 直接调用 `.unwrap()`。执行 `cargo test -p astro-agent matching_ack_drains_provisional_nonterminal_events_before_terminal_in_order` 时在目标测试前报 `E0599`，建议将两个用例改为 Tokio async test 并 `await` Session 构造。

### 验收标准

- `astro-agent` 测试目标可编译。
- 两个 manual compaction hook 用例保持原断言。

### 验证结果

- `cargo test -p astro-agent manual_compaction -- --nocapture`：通过。
- `cargo test -p astro-agent manual_pre_compact_can_stop_before_side_effects -- --nocapture`：通过。

## H-007 旧 TokenCount rollout 的未缓存输入投影为零

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Low |
| 发现日期 | 2026-08-30 |
| 发现阶段 | Usage rollout 向后兼容审计 |
| 是否已修复 | 是；桌面投影按版本标记恢复旧语义 |

### 问题与证据

旧版 `TokenCountEvent.input_tokens` 表示未缓存输入，且不存在
`input_tokens_include_cache` / `uncached_input_tokens`。当前桌面事件投影已经用版本标记恢复
`prompt_tokens`，但仍直接读取默认值为 0 的 `uncached_input_tokens`，因此历史回放会把真实的未缓存输入显示为 0。

### 验收标准

- 新事件继续直接使用显式的 `uncached_input_tokens`。
- 旧事件在版本标记缺失时回退到旧语义的 `input_tokens`。
- 同一投影测试同时覆盖新旧两类事件。

### 验证结果

`cargo test -p astro-agent token_count_projection_preserves_cache_reasoning_and_reporting_state -- --nocapture`：通过。

## H-008 Desktop 全量 TypeScript 检查被不可达分支与死变量阻塞

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Low |
| 发现日期 | 2026-08-30 |
| 发现阶段 | Agent/Harness usage UI 全量验证 |
| 是否已修复 | 是 |

### 问题与证据

- `apps/desktop/src/App.tsx` 中 `featureNav` 只可能是 `cron | loop | skills`，但标题分支同时排除这三个值，TypeScript 因此将分支内的 `featureNav` 收窄为 `never`，访问 `PAGE_META[featureNav].titleKey` 报 `TS2339`。
- `apps/desktop/src/components/settings/SkillsPanel.tsx` 计算 `enabledCount` 后没有任何消费者，`noUnusedLocals` 报 `TS6133`。

### 修复方向

删除已经不可达的 feature header 分支及其专用变量/import；删除无消费者的 `enabledCount`。不恢复已被新页面布局取代的旧标题或统计 UI。

### 验收标准

- `npx tsc --noEmit` 全量通过。
- Cron、Loop、Skills 三个 feature 页面仍由各自现有组件渲染，不改变导航行为。

### 修复与验证

已删除不可达的 feature header 分支、它的专用 import/变量及未使用的 `enabledCount`。`npx tsc --noEmit` 全量通过。

## H-009 Plugin 信息架构测试仍断言已替换的双行布局

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Low |
| 发现日期 | 2026-08-30 |
| 发现阶段 | Desktop 全量前端回归 |
| 是否已修复 | 是 |

### 问题与证据

`9bc5abc5` 已明确将 sort/search/view/refresh 控件合并到第一行，并把个人 Skill 来源改成第二行 underline tabs；但 `pluginsInformationArchitecture.test.mjs` 仍要求旧的 `plugins-context-toolbar` 包裹来源与控件，并继续匹配旧的三项来源与 `is-import` 类。`npm test` 因此有 2 项失败，实际 JSX、提交意图和当前 CSS 则一致。

### 修复方向

更新静态结构契约：第一行包含类型、scope 和操作控件；第二行包含 `installed / online / machine / updates` underline tabs。保留对旧 `plugins-primary-row`、`plugins-scope-toolbar` 和 `plugins-context-toolbar` 的否定断言，防止旧布局回流。

### 验收标准

- `pluginsInformationArchitecture.test.mjs` 全部通过。
- `npm test` 全量通过。

### 修复与验证

已将契约更新为当前两行结构，并保留对旧容器的否定断言。定向测试 6/6 通过，前端全量 419/419 通过。

## H-010 Composer 玻璃材质测试过度绑定视觉参数

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Low |
| 发现日期 | 2026-08-30 |
| 发现阶段 | Desktop 全量前端回归 |
| 是否已修复 | 是 |

### 问题与证据

`chatFloatingChrome.test.mjs` 的测试名仅要求 composer 与 sidebar 共用玻璃材质，却把边框、阴影和 focus ring 精确锁定为 `1px` / `2px 8px` / `1px`。当工作区进行不改变材质语义的轻量化视觉调整时，全量 `npm test` 仍会被这些外观常量阻断。

### 修复方向

保留对 sidebar surface token、glass edge、backdrop filter、shadow/focus outline 和 dark variant 的语义断言，放宽不影响契约的像素常量。

### 验收标准

- 测试仍能拦截透明 composer、丢失 glass edge/backdrop/shadow 或过厚 `2px` focus ring。
- `chatFloatingChrome.test.mjs` 与 `npm test` 全量通过。

### 修复与验证

已改为语义约束：边框必须为正宽度并使用 `--glass-edge`，阴影必须同时包含 `--glass-rim` 与 `--shadow-ink`，focus outline 必须为不超过 `1px` 的正宽度。定向测试 7/7 通过，前端全量 419/419 通过。

## H-011 Sidebar 样式文件尾部遗留无选择器声明

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Low |
| 发现日期 | 2026-08-30 |
| 发现阶段 | 未提交样式改动边界检查 |
| 是否已修复 | 是 |

### 问题与证据

`sidebar-polish.css` 在最后一个 `@media` 块结束后遗留两行重复的 `border-width: 0.75px;`，不属于任何选择器，是无效 CSS 声明。

### 修复方向

仅删除这两行孤立声明，不改动同一工作区中其他未提交的玻璃样式参数。

### 验收标准

- 文件结尾是完整的 `@media` 块，不再存在无选择器声明。

### 修复与验证

已只删除两行孤立声明，其他未提交样式参数保持不变；前端全量 419/419 通过。

## H-012 并行任务丢失终态语义并误触发庆祝

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Medium |
| 发现日期 | 2026-08-30 |
| 发现阶段 | 任务完成庆祝状态链审查 |
| 是否已修复 | 是 |

### 问题与证据

- Desktop 事件桥对终止回合按 `RunFinished(outcome_type) → Done` 顺序投影。
- `useParallelTasks` 只在 `run_finished` 中处理局部分支，没有保留 `outcome_type`；后续 `done` 仅根据 `terminalError` 调用 `finish("done")`。因此 `interrupt` 会被覆盖为成功，并调用 `onTaskSucceeded`。
- 主任务和并行任务在后端报告 `success` 但没有可渲染输出时，UI 会写入“空响应”错误，但成功回调仍可能触发。

### 修复方向

统一保留终态 outcome，将 `success / error / interrupt / hitl_waiting / legacy done` 显式映射为并行任务状态；只有“明确 success + 有可渲染输出 + 无终端错误”才触发庆祝。

### 验收标准

- `interrupt` 结算为 `cancelled`，`hitl_waiting` 保持未结算，两者都不庆祝。
- 错误和空响应不庆祝；仅真实成功触发。
- 用行为单元测试覆盖全部终态。

### 修复与验证

已抽取 `taskCompletion.ts` 作为主任务和并行任务共用的终态判定；并行流保留 `terminalOutcome`，显式区分 success、error、interrupt、HITL 和旧版无 outcome 的 done。新增行为单元测试覆盖成功、空响应、终端错误、interrupt、HITL 及 legacy done。

## H-013 庆祝组件重新挂载时重放旧事件

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Low |
| 发现日期 | 2026-08-30 |
| 发现阶段 | 任务完成庆祝组件审查 |
| 是否已修复 | 是 |

### 问题与证据

`TaskCompletionCelebration` 的 effect 在每次挂载时都执行，当会话中已有 `trigger > 0` 时，从设置页返回聊天、重建侧聊面板或其他 remount 都会将旧的完成事件再播放一次。

### 修复方向

将当前 trigger 作为挂载基线，仅当后续 trigger 严格递增时播放；归零或降低不播放。

### 验收标准

- 首次挂载且 trigger 已大于 0 时不播放。
- 只有严格递增触发播放；重复值、归零和降低值均不播放。

### 修复与验证

组件现在用初始 trigger 建立挂载基线，并通过 `shouldStartCompletionCelebration` 只接受严格递增的新事件。单元测试已覆盖递增、重复、归零和倒退。

## H-014 庆祝 canvas 结束后长期保留高分辨率 backing store

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Low |
| 发现日期 | 2026-08-30 |
| 发现阶段 | 任务完成庆祝性能审查 |
| 是否已修复 | 是 |

### 问题与证据

`TaskCompletionCelebration` 按面板尺寸和最高 2x DPR 创建 canvas backing store，但动画自然结束和 reduced-motion 定时结束时只调用 `clearRect`。透明像素虽被清除，大尺寸图形缓冲区仍由长驻 ChatView 持有，多个面板会叠加内存占用。

### 修复方向

引入幂等的 backing-store 释放函数，在普通动画结束、reduced-motion 定时结束、trigger 更新和组件卸载时将 canvas 回收为最小尺寸。

### 验收标准

- 所有结束路径共用同一个释放函数。
- 释放后下一次 trigger 仍会按当前容器尺寸重新初始化并播放。

### 修复与验证

已将普通动画结束、reduced-motion 定时结束和 effect cleanup 统一到幂等的 `releaseCanvasBackingStore`，回收后 backing store 缩为 `1×1`；下次 trigger 会在播放前重新按容器尺寸与 DPR 初始化。

本批验证：定向庆祝/终态测试 5/5、前端全量 424/424、`npx tsc --noEmit`、`npm run lint:css`、`npm run build`、`npm run build-storybook` 均通过；Rust 事件桥 `terminal_thread_event_maps_to_run_finished_then_done` 1/1 通过。

## 后续缺陷记录模板

## H-015 右侧停靠面板动效边界不一致

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Medium |
| 发现日期 | 2026-08-30 |
| 发现阶段 | 项目文件与侧边聊天动效审查 |
| 是否已修复 | 是 |

### 问题与证据

- 项目文件面板展开时，`ProjectFilesPanel` 的 `ResizeObserver` 会连续上报过渡中的渲染宽度；`App` 又把该宽度写入 `--chat-header-right-offset`，`.content-header--chat.has-project-files .header-actions` 据此逐帧改写 `left`。结果是本应独立稳定的模型选择器与工具栏也会随面板从右侧重新进入。
- 项目文件使用 `flex-basis / width / margin / opacity / transform` 的 300ms 布局过渡；侧边聊天在条件挂载后立即占满最终宽度，只对面板内容执行 240ms 的 `x: 12 / scale: 0.98` 动画。两者的布局变化、位移距离、缩放值和时长均不一致。

### 修复方向

将顶栏工具组从项目文件的宽度动画中解耦并固定在标题栏右侧；为侧边聊天增加独立、常驻的布局槽位，复用项目文件的 18px、0.985、300ms 曲线与 180ms 透明度节奏，面板本体只负责内容和退出生命周期。

### 验收标准

- 打开或关闭项目文件时，顶栏模型选择器和工具栏不改变定位，也不重新入场。
- 打开或关闭侧边聊天时，主聊天宽度与面板从右侧同步平滑变化，节奏与项目文件一致。
- `prefers-reduced-motion` 下不执行位移、缩放或布局过渡，仅保留可理解的状态切换。

### 修复与验证

已移除标题栏对停靠面板实时宽度的定位依赖，工具栏始终固定在标题栏右侧；侧边聊天改由常驻布局槽承载，并与项目文件统一为 18px 位移、0.985 缩放、300ms `cubic-bezier(0.22, 1, 0.36, 1)` 和 180ms 透明度过渡，窄屏及 reduced-motion 路径同步收口。

定向动效与布局测试 20/20、TypeScript、CSS lint、生产构建和 Storybook 构建均通过；Storybook 实际渲染采样中，项目文件开关全过程顶栏横坐标保持 `1056px`（漂移 `0px`），侧边聊天宽度按 `47 → 294 → 375 → 384px` 平滑展开，计算样式为 300ms 同曲线。

## H-016 聊天标题栏纵向占用偏大

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Low |
| 发现日期 | 2026-08-30 |
| 发现阶段 | 聊天标题栏视觉密度审查 |
| 是否已修复 | 是 |

### 问题与证据

聊天标题栏单独设置了 `60px` 最小高度和 `12px 16px 8px` 内边距，高于应用已有的 `--titlebar-h: 52px` 安全区；内部模型选择器与工具组均只有 34px 高，因此额外高度主要表现为空白，挤占正文首屏空间。

### 修复方向

让聊天标题栏复用 52px 标题安全区，压缩上下留白并同步右侧操作组的底部定位；保持胶囊控件与可点击区域尺寸不变。

### 验收标准

- 标题栏实际高度由 60px 收紧到 52px。
- 标题、更多菜单、模型选择器和工具组仍垂直对齐，点击热区不缩小。
- 项目文件和侧边聊天的停靠动画不受影响。

### 修复与验证

聊天标题栏现直接复用 `--titlebar-h` 的 52px 基线，上下内边距改为对称的 8px，右侧 34px 胶囊组固定在距底部 9px 的垂直中心位置；标题、模型选择器和工具组的实测中心线误差分别为 `0.01px` 与 `0px`。

定向标题栏/动效测试 10/10、TypeScript、CSS lint 与生产构建均通过；浏览器实际渲染确认标题栏从 60px 收紧到 52px，控件尺寸保持 34px。

### 2026-08-31 二次收紧

用户复核后仍认为标题栏和标题文字偏大。标题栏进一步由 52px 收紧为 48px，会话标题由 17px 调整为 16px，并将字距由 `-0.02em` 微调为 `-0.015em`，避免小字号下笔画过密；右侧模型与工具胶囊继续保持 34px 点击高度。

定向标题栏测试 11/11、TypeScript、CSS lint 和生产构建均通过；浏览器实际渲染为 48px 标题栏、16px 标题文字，标题、模型选择器和工具组中心线误差均为 `0px`。

## H-017 顶部会话标题菜单与侧栏会话菜单能力漂移

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Medium |
| 发现日期 | 2026-08-31 |
| 发现阶段 | 顶部标题栏与侧栏会话操作一致性审查 |
| 是否已修复 | 是 |

### 问题与证据

顶部标题右侧的「更多」入口在 `App.tsx` 中独立渲染 `conversation-menu-popover`，只提供“新会话 / 压缩会话 / 上下文”三项操作；侧栏会话的同类入口则在 `SidebarSessionList.tsx` 中独立渲染 `project-context-menu`，提供置顶、重命名、重新生成标题、导出、分支、移动到项目、归档与永久删除。两处菜单使用不同组件、样式类和动作实现，导致同一个会话在两个入口下的操作集合和视觉规格不一致。

### 用户影响

- 用户无法从当前会话标题直接访问侧栏已有的完整会话管理能力。
- 两套独立菜单会继续在图标、间距、禁用状态、归档态和后续新增动作上产生漂移。
- 顶部菜单的语义是“当前会话操作”，但当前混入新建会话、上下文面板等全局动作，信息架构不一致。

### 修复方向与验收标准

- 抽取单一共享会话操作菜单，顶部标题入口和侧栏会话入口都复用同一组件、同一动作顺序及同一禁用规则。
- 顶部入口必须根据当前会话的置顶、归档、项目和运行状态展示正确文案与状态。
- 重命名、标题重生成、导出、分支、移动、归档及删除均走同一执行路径；关闭、Esc、点击外部与视口约束行为一致。
- 增加静态契约测试，防止任一入口重新引入独立菜单实现。

### 修复与验证

已将完整会话操作、项目加载、运行态禁用、删除确认、视口约束和关闭行为抽取到 `SessionActionsMenu`；顶部标题和侧栏条目都传入同一份会话元数据并渲染该组件。顶部原有的独立三项菜单及其专属样式已移除，当前会话元数据 hook 同步返回标题、置顶、归档和项目状态。

- 定向会话菜单契约测试 3/3 通过，确认两个入口都使用共享组件，且 8 项动作仅由共享组件定义。
- `npx tsc --noEmit` 与 `npm run lint:css` 通过。
- `npm run build` 通过；仅保留既有 Vite 大 chunk 和动态/静态 import 提示。
- Storybook 实际交互验证得到 8 项菜单：置顶、重命名、重新生成标题、导出、分支、移动到项目、归档、永久删除；菜单尺寸 `180 × 266px`，视口内自动上翻且使用同一 `project-context-menu` 视觉规格。

## H-018 聊天标题栏阻断消息滚动视口且缺少玻璃层次

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | Medium |
| 发现日期 | 2026-08-31 |
| 发现阶段 | 聊天标题栏滚动与材质审查 |
| 是否已修复 | 是 |

### 问题与证据

`App.tsx` 将 `.content-header--chat` 作为 `.content-pane` 的普通 flex 子项放在 `.page-body--chat` 之前；标题栏虽然已收紧到 42px，但仍会先占据纵向布局空间，导致 `ChatView` 的 `.message-list` 滚动视口只能从标题栏下缘开始。与之相对，底部 `.composer-shell` 已采用绝对定位，并通过消息列表动态尾部留白实现“内容在玻璃层下滚动”的连续体验。

标题栏自身目前只有透明背景，没有复用输入框的半透明底色、玻璃描边、内高光、阴影与 backdrop blur，因此滚动内容即使延伸到顶层也缺少稳定的前后景分离。

### 用户影响

- 消息内容无法滚动到窗口顶部，顶部 42px 始终是不可利用的固定空带。
- 顶部标题栏与底部输入框使用不同的空间模型和材质语言，界面上下不一致。
- 直接取消标题栏占位而不补首屏安全间距，会让第一条消息初始状态被控件遮挡。

### 修复方向与验收标准

- 将聊天标题栏改为相对 `content-pane` 的悬浮层，不再参与页面主体的 flex 高度分配。
- 复用输入框的玻璃材质配方：半透明基底、轻量 sheen、玻璃边缘、内高光和 backdrop blur；滚动文字应能透过材质被感知。
- 主消息列表仅在初始顶部增加与标题栏等高的安全留白，滚动后内容可继续进入标题栏背后；侧边聊天不继承该留白。
- 空白区域继续支持窗口拖动和滚轮透传，标题、菜单与右侧操作控件保持可点击。
- 覆盖浅色、深色、窄屏、降低透明度和实际滚动场景。

### 修复结果

- `.content-header--chat` 已改为不占据正文高度的绝对定位玻璃层，并复用输入框的半透明基底、sheen、玻璃边缘、内高光与 backdrop blur 配方。
- 主消息滚动容器从窗顶开始，首屏使用 54px 安全留白；滚动后文字会进入玻璃标题栏背后。侧边聊天不继承该留白，定时任务浮卡和临时会话提示同步避让。
- 空白区域保持事件透传，标题、会话菜单和右侧操作区保持可点击；补充降低透明度和高对比度回退。
- `chatFloatingChrome.test.mjs` 11 项测试、TypeScript、CSS lint 与生产构建通过；Storybook 浅色/深色及真实滚动验证确认标题栏高 42px、消息视口顶点为 0，滚动时存在内容进入标题栏背后；640px 窄屏无控件重叠。

## H-019 活动组进度参数存在重复字段导致前端无法构建

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | High |
| 发现日期 | 2026-08-31 |
| 发现阶段 | H-018 生产构建回归 |
| 是否已修复 | 是 |

### 问题与证据

`MsgActivityGroup.tsx` 在组装 `chat.activityGroup.progress` 参数时，同时声明了两次 `completed`：一次取 `progress.resolved`，一次取 `progress.done`。TypeScript 报告 `TS1117: An object literal cannot have multiple properties with the same name`，使桌面前端生产构建在进入 Vite 前直接失败。

### 修复方向与验收标准

- 保留与“完成”文案语义一致的成功完成数 `progress.done`，移除重复的 `progress.resolved` 字段。
- 活动状态聚合测试、TypeScript 和生产构建恢复通过。

### 修复结果

- 已移除重复的 `progress.resolved` 映射，`completed` 唯一取值为成功完成数 `progress.done`。
- 活动呈现相关测试 17 项、TypeScript 与生产构建均通过。

```markdown
## H-NNN 标题

- 状态 / 严重度 / 发现日期 / 是否已修复
- 问题描述
- 源码证据与调用链
- 用户影响和数据风险
- 最小复现
- 建议修复方向
- 验收标准
```
