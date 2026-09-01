# Astro Agent

本地 AI 桌面工作站（阿童木）。支持智能对话、Realtime 语音会话、记忆召回、工作区与文件空间，可接入多家模型，并调用工具与 Skills 完成复杂任务。偏好设置保存在本机。

技术栈：**Rust workspace + Tauri 2 + React / Vite**。

## 环境要求

- [Rust](https://rustup.rs/)（edition 2021）
- Node.js 18+
- [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)（macOS 需 Xcode CLT）

## 快速开始

```bash
# 安装前端依赖
cd apps/desktop && npm install

# 开发模式（热更新）
npm run tauri dev

# 打包桌面应用（当前机器默认架构的全部 bundle）
npm run tauri build

# 仅打 macOS DMG / .app（需在 macOS 上执行）
npm run tauri build -- --bundles dmg
npm run tauri build -- --bundles app
```

### macOS：ARM（Apple Silicon）与 x86_64（Intel）

本机默认只打**当前 CPU 架构**。要同时支持 ARM / x86，先安装 Rust 目标：

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

然后在 `apps/desktop/` 下按架构打包（npm scripts）：

```bash
# Apple Silicon (ARM64)
npm run tauri:build:arm
npm run tauri:build:dmg:arm

# Intel (x86_64)
npm run tauri:build:x64
npm run tauri:build:dmg:x64

# Universal（一份包同时含 ARM + x86，体积更大）
npm run tauri:build:universal
npm run tauri:build:dmg:universal
```

等价 CLI：

```bash
npm run tauri build -- --target aarch64-apple-darwin
npm run tauri build -- --target x86_64-apple-darwin
npm run tauri build -- --target universal-apple-darwin
```

产物目录（按 target 区分）：

```text
# macOS
target/aarch64-apple-darwin/release/bundle/macos|dmg/...
target/x86_64-apple-darwin/release/bundle/macos|dmg/...
target/universal-apple-darwin/release/bundle/...   # universal
target/release/bundle/...                          # 本机默认架构时

# Windows（Windows / CI）
target/release/bundle/nsis/*.exe
target/release/bundle/msi/*.msi

# Linux（Linux / CI）
target/release/bundle/appimage/*.AppImage
target/release/bundle/deb/*.deb
```

图标与 DMG 资源在 `apps/desktop/src-tauri/icons/`（`icon.png` / `icon.icns` / `icon.ico` 等）；DMG 安装窗口背景为 `dmg-background.png`，在 `tauri.conf.json` → `bundle.macOS.dmg` 中配置。

Windows / Linux 的 ARM 包需在对应 ARM 机器或 CI runner 上构建（见下方 CI）；本机 Mac **不能**交叉打出 Windows/Linux 安装包。

也可在仓库根目录用 Cargo 只编译二进制（不含安装包）：

```bash
cargo build -p astro-agent --release
cargo build -p astro-agent --release --target aarch64-apple-darwin
cargo build -p astro-agent --release --target x86_64-apple-darwin
```

## CI 多平台打包

仓库已配置 GitHub Actions（见 `.github/workflows/`）：

| Workflow | 触发 | 作用 |
|----------|------|------|
| `build-tauri` | PR（相关路径变更）/ 手动 | macOS **arm64 + x86_64**、Linux x64、Windows x64，上传 Artifacts |
| `release-tauri` | 手动 / `release` 分支 / `v*` 标签 | 同上，写入**草稿** GitHub Release |

说明：

- macOS：CI 分别构建 ARM 与 Intel 两套包；若只要一份通用包，可在本机用 `npm run tauri:build:universal`。
- Tauri 需在对应系统上原生构建，无法在一台机器上交叉打出全部 OS 安装包。
- 首次使用 Release 前，在仓库 **Settings → Actions → General → Workflow permissions** 勾选 **Read and write permissions**。
- 推送到 GitHub 后，在 Actions 页点 **Run workflow** 即可试跑。

## 托盘常驻

关闭主窗口会**隐藏到系统托盘**，内嵌 backend / cron 继续运行。左键点托盘图标可恢复窗口；托盘菜单「退出 Astro」或 macOS「Astro → 退出」才会真正结束进程。

偏好设置中的界面语言（中文 / English）会同步到**原生菜单栏与托盘**文案。

定时任务（含后台 due 触发与手动「立即执行」）以及入梦完成/失败时，会弹出**系统通知**（需授予通知权限）。

## gRPC 后端（默认内嵌）

`tauri dev` / 打包后的 `.app` **默认在同进程启动 gRPC backend**（含 cron），无需另开终端。双击 APP 即可聊天。

**端口：** 未设置 `ASTRO_GRPC_ADDR` 时内嵌使用 `127.0.0.1:0`，由系统分配空闲端口，并在进程内告诉壳侧客户端（用户无感、不与其它进程抢 50051）。独立 `cargo run -p server` 仍默认 `127.0.0.1:50051`。

调试固定端口：

```bash
export ASTRO_GRPC_ADDR=127.0.0.1:50051
```

**单实例：** 再次打开 APP 不会起第二套进程/backend，而是把已有窗口拉到前台（Windows/Linux 走 single-instance 插件；macOS 另支持 Dock 再点 / Reopen）。

如需**独立进程**调试：

```bash
# 终端 1：只跑 backend（默认 50051）
cargo run -p server

# 终端 2：关掉内嵌，连外部 backend
export ASTRO_EMBED_BACKEND=0
export ASTRO_GRPC_ADDR=127.0.0.1:50051
cd apps/desktop && npm run tauri dev
```

也可在 `~/.astro/.env` 写入 `ASTRO_EMBED_BACKEND=0` / `ASTRO_GRPC_ADDR=…`。

## Realtime 语音会话

Realtime 已拆分为独立 `agent-realtime` crate，并通过统一 Thread 协议接入桌面端：

- 桌面端默认使用 WebRTC，通过 `oai-events` data channel 和媒体 track 建立双向会话；
- 支持 WebSocket 兼容传输、WebRTC SDP offer/answer，以及 ExistingCall sideband 接管；
- Azure OpenAI 使用 GA `/openai/v1`：WebSocket/sideband 采用 `api-key`，WebRTC 通过
  `client_secrets` 临时凭据与原始 `application/sdp` 协商；
- 支持 V2 GA 与显式 V3 `/live/{call_id}` 协议，Provider JSON 在 crate 内转换为 typed events；
- `gpt-realtime` 原生处理文字/音频输入输出，不要求独立转写模型；Realtime 仅暴露
  `background_agent` 与 `remain_silent` 两个内部控制动作；
- 完整 transcript、会话边界和 BEM promotion 以 `RealtimeItem` 持久化到 rollout，原始音频和 delta 不落盘；
- Codex handoff 可把语音请求转为普通 Agent turn，再按 `thinking` / `commentary` / `bem_tags` 返回 Realtime call。

设计与恢复契约见 [Realtime 子系统](./docs/realtime-subsystem.md)，Azure 来源快照与映射见
[Azure OpenAI Realtime 参考](./docs/azure/realtime/README.md)，整体 Agent 边界见
[架构总览](./docs/03-系统设计阶段/01-架构设计/01-架构总览.md)。

## 仓库结构

```text
astro/
├── Cargo.toml              # Workspace 根（26 个 crate + 1 个桌面应用）
├── crates/                 # 所有 Rust crate（扁平 agent-* 命名）
│   ├── agent-core/         # Agent 运行时核心（Session、streaming、工具路由）
│   ├── agent-types/        # 通用 DTO 与兼容投影（Message、ToolEntry、ToolExposure 等）
│   ├── agent-config/       # 分层配置原语
│   ├── agent-protocol/     # Core 协议与 canonical ResponseItem
│   ├── agent-rollout/      # JSONL append-only 历史
│   ├── agent-realtime/     # WebSocket/WebRTC/ExistingCall 与 V2/V3 会话
│   ├── agent-providers/    # 多厂商 LLM/图像 Provider（15+ 厂商）
│   ├── agent-tools/        # 工具实现 + ToolRegistry（BM25 搜索、三级暴露）
│   ├── agent-subagents/    # V2 Agent Thread 子 Agent 系统
│   ├── agent-memory/       # 记忆管理
│   ├── agent-server/       # gRPC 服务端
│   ├── agent-session/      # 会话库（SQLite WAL + FTS5）
│   ├── agent-sandbox/      # 沙箱权限控制
│   ├── agent-network-proxy/ # 受管网络代理
│   ├── agent-skills/       # Skills 管理
│   ├── agent-mcp/          # MCP 客户端
│   ├── agent-hooks/        # typed Hook 生命周期与 Command/MCP 执行
│   ├── agent-proto/        # Protobuf / tonic gRPC 契约
│   └── ...                 # 另有 8 个 crate（artifacts/usage/cron/workflow/a2ui/delegate/evolution/home）
└── apps/
    └── desktop/            # React + Vite UI + Tauri 2 壳
```

| package name | 说明 |
|-------|------|
| `astro-agent` | Tauri 桌面应用（`apps/desktop/src-tauri`） |
| `agent` | Agent 运行时核心（Session、AstroThread、streaming） |
| `server` | gRPC 服务（可独立运行；桌面壳默认同进程内嵌） |
| `providers` | Agent Responses-only 路由 + 工具/媒体 Provider 适配 |
| `tools` | 工具实现 + ToolRegistry（ToolExposure 三级暴露、BM25 搜索） |
| `subagents` | V2 Agent Thread 子 Agent 系统 |
| `memory` | 记忆管理（MEMORY.md/USER.md 快照） |
| `session` | 会话消息与账单（SQLite WAL + FTS5） |
| `usage` | 用量统计与 Tracing 洞察 |
| `sandbox` | 沙箱权限控制（PermissionProfile） |
| `network-proxy` | 受管网络代理 |
| `agent-config` | 分层配置原语 |
| `agent-protocol` | Core 协议（Op、EventMsg、TurnItem、ResponseItem） |
| `agent-rollout` | JSONL append-only 权威历史 |
| `realtime` | Realtime 运输、typed event、transcript reducer 与 Codex handoff/BEM |
| `skills` / `mcp` | 扩展能力（Skills 管理、MCP 客户端） |
| `hooks` | Plugin、Command/MCP、Gateway、Shell 生命周期钩子 |
| `proto` / `types` | gRPC 契约与公共类型 |

运行时架构见 [`docs/03-系统设计阶段/01-架构设计/12-Responses原生Agent运行时架构.md`](./docs/03-系统设计阶段/01-架构设计/12-Responses原生Agent运行时架构.md)，Realtime 见 [`docs/realtime-subsystem.md`](./docs/realtime-subsystem.md)，钩子说明见 [`docs/hooks.md`](./docs/hooks.md)。

Azure `gpt-image-2` 文档：

- [配置与使用说明](./docs/azure-gpt-image-2.md)
- [实现设计](./docs/superpowers/specs/2026-09-01-azure-gpt-image-2-design.md)

## 常用命令

```bash
# Workspace 检查
cargo check

# 跑测试（示例）
cargo test -p server

# 仅构建前端静态资源
cd apps/desktop && npm run build
```

## 许可证

私有项目，未声明开源许可。
