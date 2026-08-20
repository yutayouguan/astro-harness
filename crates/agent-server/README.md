# server

gRPC 服务端（tonic）：Thread submit/resume/subscribe RPC、ThreadHistoryBuilder 活跃 Turn 快照、per-connection 128 容量队列、慢消费者断连。run_embedded() 供 Tauri in-process 使用。

属于 [Astro Agent](../../README.md) workspace，详见根目录 `CLAUDE.md` 的 Crate Map。
