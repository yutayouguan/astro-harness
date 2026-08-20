# hooks

Astro 三总线 Hook 体系：Plugin（进程内同步回调）、Gateway（外部清单文件发现）、Shell（配置驱动的异步 shell 命令），统一通过 `HookRuntime` 聚合调度。

## 核心职责

- **Plugin Bus** -- 进程内同步钩子总线，支持注册回调、按事件名触发，返回 `HookOutcome`（Continue / Block / Skip / Modify / Rewrite 等）可拦截或变更 Agent 行为
- **Gateway Registry** -- 基于文件系统发现 `hooks/{name}/HOOK.yaml` 清单，按事件订阅外部 hook handler，支持日志 fallback 注册
- **Shell Runner** -- 从 `AstroConfig.hooks` 配置映射加载，按事件名异步执行 shell 命令
- **统一调度** -- `HookRuntime::dispatch()` 一次投递同时触达三套 transport，Plugin 决定流程走向，Gateway 和 Shell 并行通知
- **UI 时间线** -- 通过 `UiTimelineSlot` 在 Plugin Bus 上安装 UI 录制回调，每轮 chat 热替换 sender

## 模块结构

| 文件 | 职责 |
|------|------|
| `lib.rs` | `HookRuntime` 聚合句柄（plugin + gateway + shell + ui_slot）、`bootstrap_from_root()`、`dispatch()` 统一投递、集成测试 |
| `event.rs` | `HookEvent` 类型化枚举（PreToolUse / PostToolUse / PreLlmCall / SessionStart / Stop 等 20+ 事件） |
| `names.rs` | canonical 事件名常量（`PRE_TOOL_USE` / `POST_LLM_CALL` 等）、`is_mutating_hook()` 判断可影响流程的钩子 |
| `outcome.rs` | `HookOutcome` 枚举（Continue / Block / Skip / Modify / Rewrite / ReplaceText / InjectContext / KeepGoing）、`HookInput` / `HookPayload` 投递载荷 |
| `plugin.rs` | `PluginHookBus` -- 进程内同步回调总线，按事件名注册/触发/移除 handler |
| `gateway.rs` | `GatewayHookRegistry` -- 文件系统清单发现（`HOOK.yaml`）、`DiscoveredHook` / `HookManifest`、handler 注册与触发 |
| `shell.rs` | `ShellHookRunner` -- 从 HashMap 配置构建、按事件名排队异步 shell 命令、支持 schedule 查询 |
| `config.rs` | `AstroConfig` YAML 加载、`default_astro_root()` 数据根路径解析 |
| `context.rs` | `PluginContext` -- 持有 plugin bus + gateway 引用的便捷上下文 |
| `ui.rs` | `UiTimelineSlot` / `UiTimelineGeneration` / `UiHookEvent` -- UI 事件录制与时间线管理 |

## 核心类型与 API

- `HookRuntime` -- 三套 transport 的聚合句柄，`new()` / `bootstrap_from_root()` / `dispatch()` / `fire_plugin()` / `fire_gateway()`
- `PluginHookBus` -- 进程内同步回调总线，`register(event, callback)` / `fire(event, payload)` / `remove(event)`
- `GatewayHookRegistry` -- 文件发现型 hook 注册表，`discover(root)` / `register_handler(name, callback)` / `fire(event, payload)`
- `ShellHookRunner` -- 配置驱动的 shell 命令运行器，`from_map(config)` / `fire_async(event, payload)` / `scheduled()` / `has_event(name)`
- `HookEvent` -- 20+ 种事件的类型化枚举，`as_str()` 返回 canonical 名称
- `HookOutcome` -- 钩子返回值枚举，决定流程走向：Continue（放行）、Block（阻止）、Skip（跳过）、Modify / Rewrite（变更内容）
- `HookPayload` -- 投递载荷，携带 prompt / detail / tool_name 等上下文
- `UiTimelineSlot` -- UI 录制槽，每轮 chat 热替换 sender，`install()` 注册到 plugin bus

## 设计要点

- **Canonical 事件名** -- 所有 transport 统一使用 PascalCase canonical 名（如 `PreToolUse`），不接受 legacy snake_case（如 `pre_tool_call`）
- **可变与观察** -- `is_mutating_hook()` 区分可影响流程的钩子（PreToolUse / Stop / Transform* 等）与纯观察型钩子（PostToolUse / SessionStart 等）
- **投递顺序** -- Plugin 先于 Gateway 先于 Shell；Plugin 的 Block/Skip 不阻止 Gateway 和 Shell 执行（三路并行通知）
- **热替换** -- UI 时间线 sender 每轮 chat 热替换，不中断已注册的 plugin handler

## Crate 关系

| 方向 | crate |
|------|-------|
| 被依赖 | `agent`（核心运行时在每轮 LLM 调用、工具执行前后触发 hook）、`tools`（工具审批逻辑）、`server`（gRPC 服务启动时 bootstrap） |
| 无内部依赖 | 本 crate 不依赖 workspace 内其他 crate |

## 测试

```bash
# 全部测试（含 lib.rs 中的集成测试）
cargo test -p hooks

# 单个测试
cargo test -p hooks dispatch_reaches_all_transports_with_canonical_name -- --nocapture
```
