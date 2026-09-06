# 聊天 Fallback 链（主模型故障切换）

**日期:** 2026-07-14  
**状态:** 显式 fallback 已实现；Codex retry/网络恢复顺序待硬切
**实现分支:** `feat/chat-fallback-chain`  
**关联:** [Providers Hermes Profiles](./2026-07-13-providers-hermes-profiles-design.md)、[路由感知用量与费用](./2026-07-13-route-aware-usage-pricing-design.md)  
**外部参考（正文不重复品牌名）:** 参考 Agent 主循环「主模型失败 → fallback_providers」语义

> **2026-08-28 网络恢复边界：** DNS/TCP/TLS/CONNECT 建连失败不再进入本设计的
> Provider fallback 链，而按
> [Codex 网络恢复对齐设计](./2026-08-28-codex-network-recovery-alignment-design.md)
> 在同一目标、同一 Turn 内等待恢复。该变更为硬切，不保留字符串分类或 Google 私有重试。

### 实现说明

| 入口 | 状态 |
|------|------|
| 主聊天（Tauri → gRPC → `ProviderStreamer`） | 已接线 fallback 链 |
| Cron（Tauri `run_cron_job_now` / `resolve_creds_for_job`） | 已接线 `resolve_chat_targets` |
| Delegate / 子 Agent | 已下传 `chat_targets`，走 `try_stream_completion_with_fallback` |
| Providers 面板 MVP | 可编辑最多 3 条后备 |
| **Backend `cron_runner`** | 已接线：读 `providers.json` + **仅环境变量** Key 展开链（无 keyring；与 Tauri 手动跑互补） |
| **Orchestration（`orchestration_run`）** | 已接线：下传 `chat_targets`，`run_provider_loop` 走 `try_stream_completion_with_fallback` |

## 决策摘要

| 项 | 选择 |
|----|------|
| 切换粒度 | **单次 LLM 调用**临时切换；不改 `active_provider_id` / 会话默认模型（下一用户消息仍先打 primary） |
| 可切换窗口 | **仅首包前**，且当前目标的 Codex retry 预算已耗尽（429、401/403 可直接切换） |
| 配置来源 | 活跃/指定 provider 条目上显式 `fallback: [{ provider_id, model? }, …]`，存 `providers.json` |
| 覆盖入口 | 主聊天多轮 + cron + 子 Agent / 编排委派（同一 helper） |
| 实现位置 | Agent 侧 `try_stream_completion_with_fallback`；边界解析凭据后下传 `Vec<ChatTarget>` |
| 401/403 | 可 failover；MVP **不做** OAuth refresh |

## 目标

1. Primary 在首包前因限流、服务端或鉴权失败时，按配置顺序尝试后备 provider/model；明确的连接失败留在当前目标等待恢复。
2. 成功跳的真实路由写入 usage（双写路径不变）。  
3. 链耗尽时返回聚合错误；配置缺失时行为与今天一致。  
4. 主聊 / cron / delegate 共用一套尝试逻辑，避免分叉。

## 非目标

- 在 fallback 层处理流中途 partial；该恢复由 Codex-aligned sampling retry 独立负责
- 会话 / 全局 sticky 记住后备  
- OAuth / token refresh 后再重试同一 provider  
- `ApiMode::Responses` 接线、Bedrock 等新协议  
- 按「已启用厂商」自动猜链（无显式配置则无 fallback）  
- Providers 面板重度 UX（MVP：选条目 + 可选 model + 最多 3 条）

## 方案选择

在「Agent 入口轮询 / providers 层透明 failover / 仅 Tauri 解析」中采用 **Agent 入口轮询（方案 1）**：解析在边界完成，切换执行在每次 `stream_completion` / 等价 `chat_stream` 调用点。图片工具已有 primary→fallback 先例，聊天对齐同一形态。

---

## 1. 配置模型与解析

### 存储

路径仍为 `~/.astro/providers.json`（`ASTRO_MEMORY_DIR` 可覆盖根目录）。在每个 provider 条目上增加可选字段：

```json
{
  "id": "prov-primary",
  "kind": "openai",
  "model": "gpt-5.6",
  "enabled": true,
  "fallback": [
    { "provider_id": "prov-claude", "model": "claude-opus-4-8" },
    { "provider_id": "prov-deepseek" }
  ]
}
```

- `provider_id`：同文件内另一条目 id（须 `enabled`，且能解析到 API Key；Ollama 等 `AuthKind::None` 除外）。  
- `model`：可选；缺省用目标条目自身 `model`。  
- 缺省 / 空数组 / 缺字段：无 fallback。  
- 反序列化兼容：老文件无 `fallback` 照常加载；未知字段忽略。  
- MVP 上限：**最多 3** 条 fallback（UI 与解析都截断或拒绝多余项，以实现计划为准）。

### 运行时目标

```text
ChatTarget {
  provider_id,      // providers.json 条目 id（可观测）
  backend_id,       // registry / ApiMode 用（如 openai、claude）
  model,
  api_key,
  base_url,
}
```

解析规则：

1. Primary = 本次请求选用的 provider（现有 chat/cron/delegate 凭据来源不变）。  
2. 按该条目 `fallback[]` 顺序展开；解析失败（禁用、无 Key、自引用、成环）→ **跳过并打日志**，不整链作废。  
3. 去重：同一 `provider_id`（或相同 backend+model+base_url）只保留首次。  
4. **绝不写回** `active_provider_id`。

解析发生在边界（Tauri 开聊、cron 调度落地、gRPC 入站补全、父 Agent 启动子任务前）。Agent / 子 Agent **不**自行读 `providers.json`+keyring；只消费已下传的 `Vec<ChatTarget>`（至少含 primary）。

### UI（MVP）

Providers 面板：当前编辑条目下「聊天后备」——选择其它已启用条目、可选覆盖 model、可排序，最多 3 条。无配置时不干扰现有布局。

---

## 2. 错误分类与首包前语义

### 一次尝试

对链上每个 `ChatTarget`：

1. 组装该目标的 `ProviderConfig`，先执行 request retry。
2. `ConnectionFailed` 返回 sampling 层进入同目标网络恢复，不进入 failover。
3. 其他 Codex-retryable 错误在当前目标耗尽 stream retry 预算。
4. 仅在尚未向 UI 发出文本 / tool call / reasoning 等有效内容时，对符合矩阵的错误切换下一目标。
5. 一旦已向上游发出有效内容 → **本跳锁定**；其后失败不可 failover。
6. 新目标使用新 retry state；链耗尽后终止，不从 primary 重启。

### 可 failover

| 条件 | 说明 |
|------|------|
| HTTP 429 | 限流 |
| HTTP 5xx | 服务端错误 |
| HTTP 401 / 403 | 鉴权失败（不 refresh，直接下一家） |
| SSE 首事件即供应商错误且未产出内容 | 立即拒请求 |

连接/TLS/DNS/CONNECT 错误不属于可 failover 条件。请求超时走共享的有界 retry；只有最终被
分类为可 failover 的服务端错误才允许进入下一目标。

实现：`is_failover_eligible(&ProviderError) -> bool`，仅做 typed match；不保留 status/错误串匹配。

### 不可 failover

- 400 等其它 4xx（坏请求、上下文过长等）  
- 用户取消 / abort  
- DNS / TCP / TLS / CONNECT 建连失败（进入同目标网络恢复循环）
- 已吐字或已组 tool_call 后的流错误  
- 工具执行失败、HITL 取消等非 `chat_stream` 错误  

### 链耗尽

按序试完仍失败 → 聚合错误，例如：`主模型失败（429）；后备 A 失败（503）；…`（对齐图片生成多错误拼接）。

### 可观测

- `tracing`：每次切换 `warn`，含 from/to、原因。  
- 可选：多轮流非致命 `Status("临时切换到 …")`；不改活跃 provider。  
- Usage：按 **实际成功跳** 的 provider/model/base_url 走现有双写。

---

## 3. Helper 与三入口挂接

### Helper（`agent`）

```text
run_sampling_with_fallback(targets)
  for target in targets:
    run_target_with_retry(target, fresh_retry_state)
      → request retry
      → connection recovery or bounded stream retry
    if pre-content + failover eligible: continue
    else: return
```

- `ProviderStreamer` 持有完整 `targets`，但 retry state 按当前 target 创建，不包住整条链重试。
- 现有 `try_stream_completion_with_fallback` 是待重构入口，不能继续在单次 sampling 请求内直接遍历整链。
- 返回的 `ActiveTargetMeta` 供本跳 usage / 展示；下一用户轮仍从 `targets[0]` 起试。

### 入口

| 入口 | 改动 |
|------|------|
| 主聊天多轮 | 开聊时解析 `Vec` 注入 streamer；多轮内每一跳 LLM 均走 helper |
| Cron | 构造执行上下文时带同一 `Vec`；`cron_exec` 内直接 `chat_stream` 处改为 helper / streamer |
| Delegate / 编排 | `DelegateRunRequest`（或等价）增加已解析 fallback 链；父侧 resolve 后下传 |

gRPC 若自带凭据：以请求 primary 为准，再按 primary 条目上的 `fallback` 补全链。

---

## 4. 测试与验收

### 测试

- `is_failover_eligible`：429/503/401 vs 400/abort/connection/TLS/DNS
- 链：primary 失败 → secondary 成功；双失败 → 聚合错误  
- retry 顺序：当前目标耗尽后只向后移动，不从 primary 循环重启
- 首包后错误：已 yield 文本再失败 → 不进下一家  
- 解析：跳过无 key / 自引用 / 去重 / 最多 3 条  

### 验收标准

1. 配置空 fallback 时不发生模型切换；retry/网络恢复仍按新契约执行。
2. Primary 429（首包前）且后备可用 → 用户看到完整回复；默认活跃 provider 未变。  
3. 流中途失败 → 直接错误，不静默换模。  
4. Cron / delegate 在传入链时与主聊一致切换。  
5. 成功跳的 usage 落在实际 provider/model 上。
6. 前台断网不切换 Provider、不返回“全部模型尝试失败”，网络恢复后继续原 Turn。

---

## 5. 实现顺序（供计划拆分）

1. `providers.json` 字段 + 解析 `Vec<ChatTarget>` + DTO/UI 最小改动  
2. `is_failover_eligible` + helper + `ProviderStreamer`  
3. 主聊天接线 + Status/日志  
4. Cron + delegate 下传与调用替换  
5. 单测与文档交叉引用  

## 开放问题（实现期裁定）

- 解析模块最终落在 `apps/desktop/src-tauri` 抽出共享 crate，还是 `memory`/`providers` 旁新模块：以依赖方向（keyring 归属）为准，计划阶段选定。  
- 聚合错误是否本地化文案：保持中文用户可见消息即可。
