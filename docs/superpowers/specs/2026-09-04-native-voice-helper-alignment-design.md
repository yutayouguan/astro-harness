# Native Voice Helper 对齐设计

> 状态：设计审查通过，等待签名的原生 runtime 产物
> Codex 对照范围：`#41897`、`#42204`、`#42208`、`#42209`、`#42404`

## 结论

Astro 现有实时语音由 Desktop 捕获 PCM，并通过 backend 的 Realtime WebSocket 发送，已经
覆盖会话、转写、音频输出、steering、handoff 和持久化。Codex 新增的 voice helper 目前是
私有原生 runtime 的生命周期基础，不负责打开设备或传输音频，因此不能替换现有链路。

Astro 可增加 native helper 作为可选音频后端，但只有签名、可复现的三平台 runtime
产物准备完成后才允许出现在 UI；缺失 helper 时继续使用现有 WebView 音频路径。

## 进程与协议边界

```text
Desktop owner
  -> resolve packaged helper by canonical path
  -> spawn with scrubbed environment and owned process guard
  -> Hello { protocol, build_commit }
  <- Ready
  -> InitializeRuntime
  <- RuntimeReady
  -> future audio/device messages
  -> Close
  <- Closed + exit(0)
```

- stdin/stdout 使用 4-byte big-endian 长度前缀加 JSON payload。
- 生命周期帧上限 256 bytes，枚举拒绝未知字段；stdout 只能写协议帧。
- reader 必须跨 pipe chunk 保存半帧，先校验长度再分配，EOF 和超限均失败。
- 握手与关闭超时 5 秒，runtime 初始化超时 30 秒；失败时终止并回收子进程。
- helper 与 Desktop 的 build commit、protocol version 必须完全一致。
- 只从应用物理安装目录解析 helper，canonical path 必须仍位于该目录内。

## 环境与原生运行时

- 子进程采用显式环境 allowlist；不继承 API key、shell 注入变量或任意动态库搜索路径。
- GStreamer plugin/system paths 固定为空，registry 禁止更新，避免扫描用户和系统插件。
- macOS 校验 Mach-O 架构、依赖闭包、相对 install name 与签名；Linux 校验 ELF 架构、
  RUNPATH 与动态依赖；Windows 校验 PE32+ 架构、imports 与 DLL 搜索目录。
- 源归档、构建产物和 runtime projection 都必须有 SHA-256 receipt；失败只清理新 staging。
- 发布包还需完成许可证清单、macOS notarization、Windows Authenticode 和最小系统版本验收。

## 与现有 Realtime 的集成顺序

1. 先实现纯生命周期协议和 chunk-independent reader，不接触设备。
2. 加入三平台受验证 runtime 产物和安装路径解析。
3. 增加 capture/playback 消息、背压和 cancellation；音频帧不得与控制帧共用 256-byte 上限。
4. 在 `RealtimeConversationManager` 外增加 transport 选择，不改变 durable `RealtimeItem`。
5. Desktop 仅在 helper 探测成功后显示 native backend；切换失败回退 WebView，不能中断历史。

## 验收矩阵

- 分片 header、分片 payload、多个帧同 chunk、EOF、超长和非法 JSON。
- build/protocol 不匹配、初始化超时、关闭超时、父进程退出和 helper 崩溃。
- 环境变量 allowlist 与动态库搜索路径清理。
- 三平台架构/依赖/签名检查以及包内路径 symlink 逃逸。
- 连续 Realtime session、steering、handoff、取消与重启恢复保持现有顺序。

## 实施门槛

没有受验证的 native runtime bundle 时，不提交一个只会握手的空 helper 到正式包，也不在
Desktop 暴露不可用入口。该门槛与 Codex 当前源码注释一致：平台 projection 仍是开发产物，
不代表设备、音频或发布链已完成。
