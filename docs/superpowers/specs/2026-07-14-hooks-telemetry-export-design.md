# Hooks 遥测旁路导出设计（Shell → 外部 Webhook）

**日期:** 2026-07-14  
**状态:** 已批准 / 已实现  

**范围:** `HookPayload` / Shell env 贯通 `turn_id`；示例 `telemetry-webhook.sh` + 配置片段；文档说明  
**依赖:** Hooks 系统（已实现）；可观测 S1 `turn_id`（已实现）  
**相关：** [可观测性 S1](./2026-07-14-observability-alignment-design.md)、[Hooks 系统设计](./2026-07-14-hooks-system-design.md)

## 命名约束

- **代码、模块、类型、文件、用户可见文案、路径中不得出现 `hermes` / `Hermes` 字样。**
- 推荐命名：`turn_id`、`ASTRO_HOOK_TURN`、`telemetry-webhook.sh`、`ASTRO_TELEMETRY_URL`。
- 本文档可描述对接 Langfuse / OTLP **兼容端点**；实现不以某云厂商 SDK 为硬依赖。

## 背景

Astro 可观测主路径已是 SessionStore + UsageDb + 本地 `agent.log`/`errors.log` + Insights。  
原观测路线图将 **Langfuse / OTEL** 定为「经 Hooks 旁路、不编进内核」。

现状缺口：

1. `HookPayload` **无** `turn_id`（仅有可选轮次计数 `turn: Option<usize>`）  
2. Shell env 仅有 `ASTRO_HOOK_{EVENT,SESSION,DETAIL,TOOL,MESSAGE}`  
3. agent fire 站点未把 `current_turn_id` 传入载荷  
4. 仓库 **无** OTEL/Langfuse crate  

## 目标与成功标准

1. 观察型 Shell Hook 能在 env 中读到与 Usage/日志一致的 `session_id` + `turn_id`  
2. 附带可运行示例：POST JSON 到 `$ASTRO_TELEMETRY_URL`（用户自配；可指向 Langfuse ingestion / 自建网关 / OTLP HTTP 桥）  
3. **默认构建零 OTEL 依赖**；未配置 `hooks:` 时行为与今日一致  

成功手测：

- 配置 `post_tool_call` 指向示例脚本 → 工具调用后请求体含 `session_id` 与 `turn_id`  
- 未设 `ASTRO_TELEMETRY_URL` 时脚本 no-op / 退出 0，不拖垮 agent  

## 明确不做

- 在 `Cargo.toml` 默认启用 `opentelemetry` / Langfuse SDK  
- 进程内 `astro-otel` feature 直推（后续可选，不在本期）  
- Gateway `HOOK.yaml` 专用遥测插件包（可用同一脚本，文档提及即可）  
- 在 Hook 中传递完整 prompt / tool args / token 费用（避免默认外泄；本期 detail/tool 名足够做 span 骨架）  
- 改 UsageDb / Insights / SessionStore schema  

## 决策摘要

| 项 | 选择 |
|----|------|
| 导出形态 | **Shell Hook → 外部脚本/curl** |
| 关联键 | `session_id` + `turn_id`（= streaming `run_id` / `AgentLoop.current_turn_id`） |
| 载荷 | `HookPayload.turn_id: Option<String>` + `ASTRO_HOOK_TURN` |
| 推荐订阅 | `pre_llm_call`、`post_llm_call`、`pre_tool_call`、`post_tool_call`、`on_session_end` |
| 失败策略 | 异步 fire；失败 `tracing::warn`；**不**阻断主循环 |

---

## 架构

```text
AgentLoop / streaming
  current_turn_id
        │
        ▼
 HookPayload { session_id, turn_id?, tool_*, ... }
        │
        ├─ PluginHookBus（不变，可观察）
        └─ ShellHookRunner.fire_async
              env ASTRO_HOOK_SESSION / ASTRO_HOOK_TURN / ...
                    │
                    ▼
           ~/.astro/config.yaml hooks:
             post_tool_call: /path/to/telemetry-webhook.sh
                    │
                    ▼
           ASTRO_TELEMETRY_URL ──► 用户端点（Langfuse/桥/自建）
```

| 组件 | 职责 | 路径 |
|------|------|------|
| `HookPayload` | 增加 `turn_id` | `hooks/src/outcome.rs` |
| `env_from_payload` | 导出 `ASTRO_HOOK_TURN` | `hooks/src/shell.rs` |
| Agent fire 站点 | 填 `turn_id: self.current_turn_id.clone()` | `agent/src/loop_.rs`、`streaming.rs`（及其他构造 `HookPayload` 处） |
| 示例脚本 | 读 env → JSON POST | `docs/examples/hooks/telemetry-webhook.sh` |
| 文档 | 配置样例与安全注意 | `docs/examples/hooks/README.md`、`docs/hooks.md` 短节 |

---

## 契约细节

### HookPayload

```rust
pub struct HookPayload {
    pub session_id: String,
    pub turn_id: Option<String>, // 新增
    // ... 既有字段不变；turn: Option<usize> 保留（轮次计数，勿与 turn_id 混淆）
}
```

默认 `turn_id: None`；会话外 / 未进入 stream 时可为 None。

### Shell 环境变量

| 变量 | 来源 |
|------|------|
| `ASTRO_HOOK_EVENT` | 事件名 |
| `ASTRO_HOOK_SESSION` | `session_id` |
| `ASTRO_HOOK_TURN` | `turn_id`（空则省略不设，或设为空字符串——**实现选：有值才 `push`**） |
| `ASTRO_HOOK_DETAIL` | detail |
| `ASTRO_HOOK_TOOL` | tool_name（若有） |
| `ASTRO_HOOK_MESSAGE` | message（若有） |

不新增把 `tool_args` / `tool_result` 全量塞进 env（体积与密钥风险）；需要时可后续加 `ASTRO_HOOK_PAYLOAD_JSON` 文件落盘选项（**本期不做**）。

### Fire 站点贯通

凡 `HookPayload { session_id: ... }` 字面量：增加

```rust
turn_id: self.current_turn_id.clone(), // 或 agent.current_turn_id().map(str::to_string)
```

静态/`Default` 构造保持 `None`。测试中 `HookPayload` 字面量补 `turn_id: None`。

### 示例脚本行为（telemetry-webhook.sh）

1. 若 `ASTRO_TELEMETRY_URL` 未设 → `exit 0`  
2. 组装 JSON：`{ "event", "session_id", "turn_id", "tool", "detail", "ts" }`  
3. `curl -sS -m 4 -X POST -H "Content-Type: application/json"` + 可选 `Authorization: Bearer $ASTRO_TELEMETRY_TOKEN`  
4. curl 失败 → `exit 0`（避免 hook 失败噪音放大；可 `STDERR` 一行）；或非零由 Shell runner warn（二选一，**推荐脚本恒 exit 0**）

### config 样例

```yaml
hooks:
  post_tool_call: '"$HOME/.astro/hooks/telemetry-webhook.sh"'
  post_llm_call: '"$HOME/.astro/hooks/telemetry-webhook.sh"'
  on_session_end: '"$HOME/.astro/hooks/telemetry-webhook.sh"'
```

用户需自行 `chmod +x` 并复制脚本到可执行路径；也可用绝对路径。

---

## 分期

### P1（内核契约）

- `HookPayload.turn_id` + Shell `ASTRO_HOOK_TURN`  
- Agent / streaming fire 贯通  
- 单元测试：`env_from_payload` 含 TURN；payload 默认兼容  

### P2（示例与文档）

- `telemetry-webhook.sh`  
- 更新 `config.yaml.snippet`、`docs/examples/hooks/README.md`、`docs/hooks.md` 一小节  
- 安全说明：勿在 hook 中打印 API key；勿默认上传全文  

---

## 测试计划

| 层 | 用例 |
|----|------|
| unit (hooks) | `env_from_payload`：有 `turn_id` 时出现 `ASTRO_HOOK_TURN`；无则不出现该键 |
| unit/compile (agent) | 所有 `HookPayload` 构造编译通过 |
| 手测 | 挂脚本 + mock HTTP（如 `nc` / webhook.site）见 JSON |

---

## 风险

| 风险 | 缓解 |
|------|------|
| `turn` vs `turn_id` 混淆 | 文档与字段注释标明；env 只用 `ASTRO_HOOK_TURN` 表示 id |
| Hook 泄露敏感 tool_args | 本期不入 env |
| 慢/挂起脚本 | 既有 5s timeout + async spawn |
| 用户期望真实 OTLP protobuf | 脚本发 JSON 到桥接层；文档写明「兼容端点 / 自建桥」 |

## 与后续（进程内 OTEL）的关系

若未来做 Cargo feature 直推：复用同一 `HookPayload.turn_id` 字段与 fire 站点；**另开 spec**。本期 Shell 路径不被该 feature 阻挡。
