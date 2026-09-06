# types

跨 crate 共享的基础类型库：模型路由目标、工具描述、凭证、权限配置、SQLite 协议与 tool spill 机制。Agent history 由 `agent-protocol::ResponseItem` 定义，不在本 crate 重复建模。

## 核心职责

- 提供工具元数据类型（`ToolEntry` / `ToolSpec` / `ToolName`）与 MCP 工具注解
- 封装 LLM 凭证（`ModelCredentials`）、模型调用目标（`ModelTarget`）与辅助任务目标链
- 定义跨 Provider 工具调用数据（`model_tool::ToolCall`）与 Google thought signature 合并规则
- 定义交互模式枚举（`InteractionMode`：Agent / Plan）
- 提供权限配置体系（`PermissionProfile` / `SandboxMode` / `SessionPermissions`）
- 封装 tool call 解析与累积器（`ToolCallAccumulator` / `ToolCallDelta`）
- 实现 tool spill 机制：超大工具结果落盘，provider 视图用 stub 替代
- 提供 SQLite WAL 打开协议（`open_wal`）与通用存储 trait
- 定义媒体资产类型（`MediaAsset` / `MediaKind` / `MediaRef`）
- 网络策略审批协议（`NetworkPolicyDecision` / `NetworkApprovalProtocol`）
- 系统通知与重要事件推送（`ImportantNotice` / `ImportantKind`）

## 模块结构

| 文件 | 职责 |
|------|------|
| `model_tool.rs` | 模型工具调用数据与 Google thought signature 辅助函数 |
| `tool_entry.rs` | `ToolEntry` / `ToolSpec` / `ToolName` — 工具元数据、MCP 注解、审批路由 |
| `tool_call.rs` | `ToolCallAccumulator` / `ToolCallDelta` / `ParsedToolCall` — 流式 tool call 解析 |
| `tool_output.rs` | `ToolOutput` — 工具执行结果封装 |
| `tool_spill.rs` | Tool spill 机制：`write_tool_spill` / `make_spill_view` / `make_prune_view`、阈值常量 |
| `tool.rs` | 工具相关辅助类型 |
| `model_target.rs` | `ModelTarget` — 含 provider/model/api_key/base_url 的模型调用目标 |
| `auxiliary_target.rs` | `AuxiliaryTask` 枚举（Dreaming/Compaction/SmartApproval/TitleGen/ToolCompress）与目标链 |
| `credentials.rs` | `ModelCredentials` / `ImageGenCreds` / `ImageGenParts` / `ImageGenTargets` |
| `model_spec.rs` | `ModelSpec` / `ModelRole` — Agno 风格模型声明 |
| `interaction_mode.rs` | `InteractionMode` 枚举：Agent / Plan |
| `permissions.rs` | `PermissionProfile` / `SandboxMode` / `SessionPermissions` / `PermissionsConfig` — 权限体系 |
| `network_policy.rs` | `NetworkPolicyDecision` / `NetworkApprovalProtocol` / `NetworkPolicyAmendment` |
| `approval.rs` | `ApprovalAction` / `ApprovalDecision` / `ApprovalMode` — 通用审批枚举 |
| `media.rs` | `MediaAsset` / `MediaKind` / `MediaRef` — 媒体资产与 sidecar 提取 |
| `sqlite.rs` | `AstroDb` / `SqliteStore` — SQLite WAL 打开协议 |
| `text.rs` | `truncate_utf8` / `truncate_chars` / `truncate_tool_result` — 文本截断工具 |
| `title.rs` | `sanitize_title` — 会话标题清洗 |
| `error.rs` | 统一错误类型 |
| `grpc_addr.rs` | gRPC 地址解析与运行时绑定 |
| `notify.rs` | `ImportantNotice` / `ImportantKind` — 系统通知与推送 |

## 核心类型与 API

- `ModelTarget` — 模型调用目标：`provider_id` / `backend_id` / `model` / `api_key` / `base_url`
- `model_tool::ToolCall` — Provider 无关的工具调用 id/name/arguments 容器
- `AuxiliaryTask` — 五类辅助任务枚举，各有独立 fallback 链
- `ModelCredentials` — LLM 凭证聚合：provider / model / api_key / base_url
- `ModelSpec` / `ModelRole` — Agno 风格模型声明与角色绑定
- `ToolEntry` — 工具元数据：name / description / schema / needs_confirmation / mcp_approval
- `ToolCallAccumulator` — 原生流式 tool_call_delta 累积器；自由文本不参与工具识别
- `ToolOutput` — 工具结果封装，支持文本与结构化输出
- `InteractionMode` — 交互模式：`Agent`（全功能，包含纯问答）/ `Plan`（只读规划）
- `PermissionProfile` — 权限配置集：预设（read-only / workspace / danger）或自定义
- `SandboxMode` — 沙箱模式：`ReadOnly` / `WorkspaceWrite` / `DangerFullAccess`
- `open_wal(path)` — 以 WAL 模式打开 SQLite，统一 busy_timeout 与 journal 配置
- `write_tool_spill` / `make_spill_view` — 超大工具结果落盘与 stub 视图生成
- `DEFAULT_SPILL_THRESHOLD_BYTES` — spill 触发阈值（默认 ~128KB）

## Crate 关系

| 方向 | crate | 说明 |
|------|-------|------|
| 被依赖 | `agent`（agent-core） | 模型目标、工具、凭证、交互模式等核心类型 |
| 被依赖 | `tools`（agent-tools） | ToolEntry、ToolCallAccumulator、审批类型 |
| 被依赖 | `providers`（agent-providers） | 间接使用 ModelTarget、ModelCredentials |
| 被依赖 | `memory`（agent-memory） | 权限与共享配置类型 |
| 被依赖 | `session`（agent-session） | SQLite 协议；会话内容类型来自 `agent-protocol` |
| 被依赖 | `sandbox`（agent-sandbox） | SandboxMode、NetworkPolicyDecisionPayload |
| 被依赖 | 几乎所有 `agent-*` crate | 作为 workspace 的类型基石 |
| 外部依赖 | `serde` / `serde_json` | 序列化 |
| 外部依赖 | `rusqlite` | SQLite 绑定 |

## 关键不变量

1. **零业务逻辑**：本 crate 仅定义类型与简单转换，不包含运行时逻辑或 I/O 操作
2. **history 归属**：不在本 crate 恢复通用 `Message`；Agent history 唯一类型是 `agent-protocol::ResponseItem`
3. **tool spill 阈值**：工具结果 >= `DEFAULT_SPILL_THRESHOLD_BYTES` 时必须落盘，provider view 使用 stub
4. **MCP 工具身份**：模型侧使用原生 `namespace=mcp__{server_id}` + `name={tool_name}`；Hub 内部执行键仍为 `mcp__{server_id}__{tool_name}`
5. **MAX_MODEL_FALLBACKS**：单条 fallback 链最多允许的备用目标数量

## 测试

```bash
cargo test -p types
```
