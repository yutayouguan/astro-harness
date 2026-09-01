# OpenAI Realtime API 参考

本目录保存 OpenAI 官方 Realtime API Reference 的离线 Markdown 快照，供 Astro
Realtime 协议实现、事件解析和兼容性审查使用。

| 本地文档 | 官方来源 | 内容 |
| --- | --- | --- |
| [realtime-api-reference.md](realtime-api-reference.md) | [OpenAI Realtime API Reference](https://developers.openai.com/api/reference/resources/realtime) | Realtime domain types、client/server events、session、conversation、transcription 与调用示例 |

快照拉取日期为 2026-09-01，正文来自官方提供的
[`realtime.md`](https://developers.openai.com/api/reference/resources/realtime.md)。
接口行为与可用模型可能更新，实施新能力前应同时核对在线版本。

相关实现：

- [Realtime 子系统设计](../../realtime-subsystem.md)
- `crates/agent-protocol/src/realtime.rs`
- `crates/agent-realtime/src/wire.rs`
- `crates/agent-realtime/src/parser.rs`
- `crates/agent-realtime/src/manager.rs`
