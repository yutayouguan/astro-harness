# Realtime 子系统设计

> 状态：已实现
>
> 更新：2026-09-04
>
> 对应 crate：`crates/agent-realtime`
>
> 适用 wire 协议：Astro `v2`（OpenAI/Azure GA）/ Astro `v3`（Codex live）

## 目标与边界

Astro Realtime 是版本化的会话子系统，不是 Agent 循环中的特殊分支。
`agent-realtime` 拥有传输协商、Provider wire 解码、typed event、handoff wire
与 transcript reducer；`agent-core` 继续拥有普通 model turn、工具执行和
rollout 写入时机。

## 系统边界

```text
Desktop WebRTC media/data channel
             |
             | SDP offer/answer
             v
agent-server / Tauri bridge
             |
             v
agent-realtime
  transport: websocket | webrtc | existing_call
  protocol:  v2 OpenAI GA | v3 frameless/live
  events:    provider JSON -> RealtimeEvent
  handoff:   request -> agent turn -> context append/complete
             |
             +---- live EventMsg ----> UI
             |
             +---- RealtimeItem -----> rollout JSONL
```

`agent-realtime` 只依赖 `agent-protocol`。`agent-core` 将 typed handoff 请求适配为
普通 turn，并持久化 history projection。Provider 原始 JSON 不跨越 crate 边界。

## 传输契约

| transport | 媒体路径 | Server 路径 | start 成功边界 |
| --- | --- | --- | --- |
| `websocket` | Core 发送 PCM16 并接收音频事件 | `/v1/realtime?model=...` | 收到 `session.created/updated` |
| `webrtc` | 浏览器 media track + `oai-events` data channel | `POST /v1/realtime/calls` 后使用 call id 连接 sideband | 获得 SDP answer 与 call id；sideband 后台接入 |
| `existing_call` | 由现有 call 拥有者维持 | 只连接 call id 对应的 sideband | 本地会话接管已启动，连接失败通过 typed closed/error 收敛 |

WebRTC 不在返回 SDP answer 前同步等待 sideband，避免 Provider 等待浏览器
`setRemoteDescription` 时形成互等。ExistingCall 不允许再注入 instructions、startup
context 或 initial items，因为这些属于原 call owner。

V3 sideband 使用有界指数退避（最多 5 次，200 ms 起步，5 s 封顶）。V2
丢失连接后直接关闭，避免重放非幂等的音频帧或 `response.create`。

### Azure OpenAI GA 适配

Azure target 由 `backend_id = "azure"` 显式选择，不按 URL 或密钥形态推断。资源根地址
规范为 `/openai/v1`，并拒绝带日期型 `api-version` 的旧 preview URL。模型字段在这里是
Azure deployment name。

- WebSocket 与 ExistingCall sideband 使用 `api-key` 请求头；
- WebRTC 先以长期 API Key 调用 `/realtime/client_secrets`，再以返回的临时 Bearer
  token 将原始 `application/sdp` 发到 `/realtime/calls?webrtcfilter=on`；
- `Location` 同时支持 path call id 与 `?call_id=` 形态；
- Azure WebRTC/ExistingCall 的首次 sideband 接入使用有界重试；连接成功后的 V2
  断线仍直接关闭，不重放非幂等事件；
- Azure 只走 V2 GA，V3 `/live/{call_id}` 保持 Codex 专用；
- 长期密钥和临时 token 都不进入浏览器，浏览器只接收 SDP answer。

Microsoft Learn 离线快照及逐项实现映射见
[Azure OpenAI Realtime 参考](azure/realtime/README.md)。

## 版本契约

这里的 V2/V3 是 Astro `RealtimeVersion` 的 **wire 协议代号**，不是模型名称、模型代际，
也不是 OpenAI Realtime 产品的 GA/Beta 版本号。三层配置彼此独立：

| 层级 | 示例 | 决定什么 |
| --- | --- | --- |
| 模型 | `gpt-realtime`、Azure deployment name | 由哪个模型处理文字/音频 |
| 传输 | `websocket`、`webrtc`、`existing_call` | 媒体和控制事件如何连接 |
| Astro wire 协议 | `v2`、`v3` | endpoint、事件结构和 delegation 编码 |

例如 `model = "gpt-realtime"`、`transport = "webrtc"`、`version = "v2"` 表示通过
WebRTC 使用公开 GA Realtime 契约调用该模型。更换模型不会自动切换 wire 协议；选择
`v3` 必须是显式操作，并且只允许走 Codex `/live` 能力。

- Astro V2 是默认公开 GA 协议映射，使用 `session.update`、conversation item、
  `response.create` 与 GA transcript/audio 事件。
- Astro V3 必须显式选择，使用 frameless `/live/{call_id}` sideband、
  `session.context.append`、delegation 事件与 BEM channel，不会对公开模型暗中升级。
- V3 context append 以 500 UTF-8 字节为上限分片，不在多字节字符中间切断。

OpenAI 官方公开语音 Agent 会话使用 `/v1/realtime`；参见
[OpenAI Realtime 指南](https://developers.openai.com/api/docs/guides/realtime)。文档中出现的
OpenAI 产品或模型代际名称与 Astro 的 `RealtimeVersion::{V2,V3}` 没有枚举对应关系。

`gpt-realtime` 原生接收文字或音频并可输出文字或音频。Astro 不要求、也不公开
`realtime_transcription_model`：独立输入转写只是字幕旁路，不是模型理解音频的前置条件。
V3 按 Codex 的原生 delegation 协议工作；公开 GA 会话则通过等价的内部函数工具桥接。

## Typed events

Provider 事件在 parser 层收敛为 `RealtimeEvent`：

- session：`SessionUpdated`；
- input：`speech_started`、transcript delta/done；
- output：transcript delta/done、`AudioOut`；
- response/item：created、cancelled、done；
- delegation：`HandoffRequested`、`NoopRequested`；
- failure：`Error`。

gRPC/Tauri/live stream 只传输这些 typed events；Provider 字段变化只需在 V2/V3
parser 内处理。

## Handoff 与 BEM

typed `HandoffRequested` 会预先订阅目标 turn 的事件 tap，然后启动或 steer 普通
Astro turn，避免丢失首个 delta。turn 的 assistant delta 以 200 ms 窗口合并后
回传 active handoff：

- `thinking`：不指定 channel 的 context append；
- `commentary`：显式进入 commentary channel；
- `bem_tags`：增量解析 `[ANALYSIS]`、`[COMMENTARY]`、`[FINAL]` 及自定义
  prefix，将可播放内容投影到 speakable channel。

V2 用 `[BACKEND]` user message 和 function output 完成 handoff；V3 用
`delegation.context.append` / `delegation.complete`。turn 进入终态后只发送一次
completion。

Realtime 会话只暴露两个窄范围内部动作：

- `background_agent`：把用户原始请求交给普通 Agent。V2 将其注册为 function tool，
  V3 将同一语义编码为 `delegation: { type: "client" }` 与 `delegation.created`；Core
  统一使用 `TurnInputMode::StartOrSteer`，空闲时启动任务，忙碌时 steering 当前任务。
- `remain_silent`：无需语音反馈时保持安静。V2 收到调用后返回空的
  `function_call_output`，但不发送 `response.create`；V3 原生允许不产生输出，不伪造
  `/live` 协议没有定义的 function tool。

## Transcript 持久化

原始 delta 保持 transient，音频不落盘。`RealtimeHistory` reducer 只写入：

- session started；
- 完整 user/assistant transcript segment；
- 将普通 turn item 与 Realtime session 关联的 BEM promotion；
- 带 `ended` / `failed` outcome 的 session closed。

这些事实以 `RolloutItem::RealtimeItem` 追加到 JSONL，通过
`agent_rollout::realtime_history()` 重建，既保持确定性，也不会重复存储每个
Provider delta。关闭时可选择保留或丢弃未完成的 transcript tail。

重复会话的边界顺序是显式契约：若新 `start` 到来时 reducer 仍有活跃 session，
先按首次活跃 role 顺序封口未完成 transcript，再写入旧 session closed，最后写入
新 session started。`agent_rollout::realtime_history()` 按 append order 恢复多个 session，
不合并、不重排边界。

handoff promotion 会先封口当前 user/assistant transcript，再追加 BEM item，
因此重启后仍保留完整事件顺序。LLM 用量另以 `TokenUsageRecord` 持久化
latest/cumulative 和 compaction checkpoint；resume 恢复最新记录，fork 不复制父
Thread 的累计值。

## 流控、失败与安全

- command/event channel 都是有界队列；音频队列满时丢弃当前帧，不阻塞录音线程。
- 单帧最大 1 MiB，且必须是 24 kHz、mono、PCM16。
- 正常 WebSocket close 不触发重连；仅 V3 异常 sideband 中断使用有界重试。
- API key 和 `ModelTarget` 只存在当前 Realtime connection，不改写普通文本回合路由。
- 一个 `Session` 同时只保留一个 active Realtime conversation；新会话不会静默覆盖活跃会话。

## 公共接入面

```text
React useRealtimeConversation
  -> Tauri start_realtime_conversation
  -> gRPC RealtimeConversationStartRequest
  -> AstroThread::submit(RealtimeConversationStart)
  -> agent-realtime::RealtimeConversationManager
```

前端默认 WebRTC，仍可显式选择 WebSocket；ExistingCall 路径只接管 sideband，
不重新开启本地录音。Tauri/gRPC DTO 暴露 transport、SDP/call id、version、
handoff mode、BEM prefixes 和 transcript-tail 策略。

## Native voice helper 边界

Astro 当前生产路径由 WebView `getUserMedia` / WebRTC 或 PCM WebSocket 完成采集与播放，
没有打包 native voice helper。Codex 当前新增的 helper 仍是 lifecycle/runtime projection
基础：长度前缀控制帧、同 build 握手、初始化与关闭，不承载设备或音频。

因此 Astro 不暴露一个只会握手的空入口。未来接入必须同时满足：包内 canonical path、
协议/build 完全匹配、分片无关的有界 frame reader、子进程环境白名单、三平台 runtime
依赖/签名校验，以及失败时回退现有 WebView 路径且不改变 durable `RealtimeItem`。完整设计见
[Native Voice Helper 对齐设计](superpowers/specs/2026-09-04-native-voice-helper-alignment-design.md)。

## 验证矩阵

- `agent-realtime`：endpoint shaping、provider-specific auth、OpenAI multipart SDP、Azure
  client-secret/raw SDP、WebSocket handshake、V2/V3 parser、
  UTF-8 分片、BEM parser、history reducer。
- `agent-protocol` / `agent-rollout`：typed event serde、durable policy 和 history 重建。
- 多 session：旧 transcript →旧 closed →新 started 的顺序，以及 rollout 跨 session 边界原样恢复。
- Desktop：WebRTC 默认路径、`oai-events`、SDP answer、ExistingCall 不启动本地录音。
- 组合检查：`agent` + `server` + `astro-agent`，并对共享工作区中的非 Realtime
  迁移失败单独归因。

## 实现索引

- OpenAI 官方协议快照：`docs/openai/realtime/`
- Azure 官方资料与实现映射：`docs/azure/realtime/`
- 协议类型：`crates/agent-protocol/src/realtime.rs`
- 传输与重连：`crates/agent-realtime/src/manager.rs`
- V2/V3 解码：`crates/agent-realtime/src/parser.rs`
- Provider wire 编码：`crates/agent-realtime/src/wire.rs`
- BEM 增量解析：`crates/agent-realtime/src/bem.rs`
- transcript reducer：`crates/agent-realtime/src/history.rs`
- Agent handoff：`crates/agent-core/src/runtime/submission_loop.rs`
- gRPC/Tauri/UI：`agent-proto`、`agent-server`、`apps/desktop/src-tauri`、
  `apps/desktop/src/hooks/chat/useRealtimeConversation.ts`
