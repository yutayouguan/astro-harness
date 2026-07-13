# Astro Agent

本地 AI 桌面工作站（阿童木）。支持智能对话、记忆召回、工作区与文件空间，可接入多家模型，并调用工具与 Skills 完成复杂任务。偏好设置保存在本机。

技术栈：**Rust workspace + Tauri 2 + React / Vite**。

## 环境要求

- [Rust](https://rustup.rs/)（edition 2021）
- Node.js 18+
- [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)（macOS 需 Xcode CLT）

## 快速开始

```bash
# 安装前端依赖
cd frontend && npm install

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

然后在 `frontend/` 下按架构打包（npm scripts）：

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

图标与 DMG 资源在 `frontend/src-tauri/icons/`（`icon.png` / `icon.icns` / `icon.ico` 等）；DMG 安装窗口背景为 `dmg-background.png`，在 `tauri.conf.json` → `bundle.macOS.dmg` 中配置。

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

## 可选：独立 gRPC 后端

桌面端默认内嵌业务逻辑；如需单独跑后端服务：

```bash
cargo run -p backend
```

默认监听 `127.0.0.1:50051`，可用环境变量覆盖：

```bash
export ASTRO_GRPC_ADDR=127.0.0.1:50051
```

## 仓库结构

```text
astro/
├── Cargo.toml              # Workspace 根
├── frontend/               # React + Vite UI
│   └── src-tauri/          # Tauri 壳（crate: astro-agent）
├── agent/                  # Agent 循环、流式输出、工具编排
├── backend/                # gRPC 服务
├── providers/              # 模型供应商
├── memory/                 # 本地记忆 / 工作区（SQLite）
├── skills/                 # Skills
├── tools/                  # 工具实现
├── mcp/                    # MCP 客户端
├── permissions/            # 权限策略
├── proto/                  # Protobuf / tonic
└── common/                 # 共享类型与错误
```

| Crate | 说明 |
|-------|------|
| `astro-agent` | Tauri 桌面应用（`frontend/src-tauri`） |
| `agent` | 对话与工具调用核心 |
| `backend` | 独立 gRPC 入口 |
| `providers` | LLM / 图像等供应商适配 |
| `memory` | 记忆、工作区、日志 |
| `skills` / `tools` / `mcp` | 扩展能力 |
| `permissions` | 访问控制 |
| `hooks` | Plugin / Gateway / Shell 三套生命周期钩子 |
| `proto` / `common` | 协议与公共库 |

钩子说明见 [`docs/hooks.md`](./docs/hooks.md)。

## 常用命令

```bash
# Workspace 检查
cargo check

# 跑测试（示例）
cargo test -p backend

# 仅构建前端静态资源
cd frontend && npm run build
```

## 许可证

私有项目，未声明开源许可。
