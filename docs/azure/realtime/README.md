# Azure OpenAI Realtime 参考与 Astro 映射

本目录保存 2026-09-01 拉取的 Microsoft Learn 中文页面快照，并记录 Astro
Realtime 对 Azure OpenAI GA 协议的实现映射。用户提供的 WebRTC URL 重复一次，
本地只保留一份快照。

## 来源快照

| 本地文档 | 原始页面 | 用途 |
| --- | --- | --- |
| [realtime-audio.md](realtime-audio.md) | [Realtime 音频概述](https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/realtime-audio) | 连接方式、模型、GA 会话与事件契约 |
| [realtime-audio-webrtc.md](realtime-audio-webrtc.md) | [Realtime 音频 WebRTC](https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/realtime-audio-webrtc) | `client_secrets`、SDP 协商和 observer/sideband |

页面正文是离线参考快照；代码片段和正文已保留。由于 Microsoft Learn 页面由浏览器
动态渲染，本次采用文本抓取回退，部分站内链接仍是相对链接或 `about:blank`，阅读最新
内容时请打开上表中的原始页面。

## Astro 实现映射

Azure provider 通过 `backend_id = "azure"` 进入显式的
`RealtimeApiFlavor::AzureOpenAi` 路径，不根据域名或密钥格式猜测。

```text
Azure provider target
  -> 规范化资源根地址为 /openai/v1
  -> WebSocket: /realtime?model=<deployment> + api-key
  -> WebRTC:
       POST /realtime/client_secrets + api-key + session JSON
       POST /realtime/calls?webrtcfilter=on
            + ephemeral Bearer + raw application/sdp
       Location -> call_id
       WSS /realtime?call_id=<id> + api-key
  -> Provider JSON -> RealtimeEvent -> Thread event / rollout
```

关键契约：

- `model` 对 Azure 表示部署名；默认模型仍由 Astro provider 配置决定。
- 只接受 GA `/openai/v1` 形态，不附加日期型 `api-version`；资源根地址会被规范化。
- Azure 当前使用 V2 GA wire；V3 `/live/{call_id}` 是 Codex 专用扩展，不会发送到 Azure。
- 浏览器只获得 SDP answer，不获得长期 API Key 或临时 secret。
- WebRTC calls 启用 `webrtcfilter=on`，限制 data channel 暴露的服务端事件；完整控制与
  transcript 继续走后端 sideband。
- WebRTC/ExistingCall 的首次 sideband 接入允许有界退避，以覆盖浏览器应用 SDP answer
  前的短暂不可用；一旦 V2 sideband 建立，断线不会重放音频或 `response.create`。
- 音频输入仍遵守 Astro 的 mono PCM16、24 kHz、单帧不超过 1 MiB 约束。

实现位置：

- `crates/agent-realtime/src/manager.rs`：Azure endpoint、鉴权、client secret、SDP 与 sideband。
- `crates/agent-realtime/src/wire.rs`：V2 `session.update`、音频和文本事件编码。
- `crates/agent-core/src/runtime/submission_loop.rs`：从 provider backend 映射 Realtime API flavor。
- [Realtime 子系统设计](../../realtime-subsystem.md)：跨 Provider 的完整生命周期与持久化契约。

## 当前范围

已实现对 Azure API Key 配置的 WebSocket、WebRTC 和 ExistingCall sideband 支持。
Microsoft Entra ID 可用于 Azure 原生接口，但 Astro provider 配置目前没有独立的凭据类型
字段，因此不会把普通 API Key 自动当成 Entra access token；后续应通过显式 credential
kind 接入，而不是依赖字符串前缀判断。
