# Astro Harness 隐私说明

Astro Harness 是本地优先的桌面应用：会话、记忆、文件索引、日志和配置默认都存放在本机
`~/.astro/` 目录下，应用本身不接入自建的服务端账号体系，也不做使用行为上报。

## 会在本机保存什么

- 会话、rollout、线程检查点与附件：`~/.astro/sessions/`
- 记忆、人格文件（SOUL/USER/MEMORY）与日记：`~/.astro/workspace/`
- 文件索引与知识库：`~/.astro/artifacts/`
- 用量统计与日志：`~/.astro/usage/`、`~/.astro/logs/`
- 全局设置与凭证入口：`~/.astro/config.toml`、`~/.astro/.env`

## 什么时候会联网

- **模型与工具调用**：使用某个 Provider（OpenAI、Azure、Google、DeepSeek、MiniMax、
  Anthropic 等）时，对话内容、工具结果和你主动附加的文件会发送到该 Provider 以完成请求。
  数据处理方式由该 Provider 的政策决定，与本应用无关。
- **内置浏览器与网页工具**：只有在你或 Agent 主动发起访问时才会连接对应站点。
- **自动更新**：检查更新时仅向公开资产仓库
  `github.com/yutayouguan/astro-harness` 请求 `latest.json`，不上报任何本机信息。
- **MCP / 插件**：只有你显式配置的 MCP 服务或扩展才会被连接。

应用不含遥测与崩溃上报。若后续接入任何统计或上报能力，会在此文件与设置页中同步说明，
并提供关闭开关。

## 凭证

Provider 密钥等敏感信息保存在本机（`~/.astro/.env` 或系统钥匙串）。这些内容不会随
安装包分发，也不会被上传到本项目的发布仓库。

## 删除数据

卸载应用不会自动删除 `~/.astro`。需要清理时，请在设置页使用存储清理能力，或在确认
备份后手动删除该目录。
