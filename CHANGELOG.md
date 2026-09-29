# 更新日志

项目从 0.2.0 起开源（MIT OR Apache-2.0 双许可）；安装包、更新签名与 `latest.json`
直接发布在源码仓库 `yutayouguan/astro-harness` 的 Releases。0.1.0 是闭源阶段的
早期二进制，只在旧资产仓库留存。

## [Unreleased]

## [0.2.0] - 2026-09-29

### 变更

- **更名为 Astro Harness**（原 Astro Agent）：应用名、窗口标题、安装包、更新地址与
  仓库同步更名；bundle identifier 与 `~/.astro` 数据根保持不变，老用户凭证与数据不受影响。
- **代码开源**：MIT OR Apache-2.0 双许可，发布不再需要跨仓库 token，改用仓库自带的
  `GITHUB_TOKEN` 把安装包与 `latest.json` 直接发在本仓库 Releases。
- 提供商接入收窄为支持 Responses API 的厂商：OpenAI、DeepSeek、Azure OpenAI、OpenRouter、
  百炼、MiniMax，以及自定义 Provider；不再提供 Claude / Google / 智谱 / Ollama 等入口，
  历史配置与媒体能力保持可用。

### 修复

- 首次启动引导的密钥字段新增「获取密钥」入口（此前只有报错后才有官方控制台入口）。

## [0.1.0] - 2026-09-28

### 新增

- 首次对外发布：macOS（Apple Silicon / Intel）、Windows x64、Linux x64 安装包。
- 应用内签名更新：关于页可检查、下载并安装新版本。

### 分发

- 引入可选系统代码签名链路：证书到位时 macOS 走 Developer ID 签名 + 公证、
  Windows 走 Authenticode 签名；当前未采购证书，安装包按未签名发布并在 Release
  说明中给出首启提示。
- 发布工作流增加发布前置检查（版本号一致性、tag 对齐、密钥完整性）与发布后校验
  （`latest.json` 必须覆盖全部四个平台条目）。

[Unreleased]: https://github.com/yutayouguan/astro-harness/releases
[0.2.0]: https://github.com/yutayouguan/astro-harness/releases/tag/v0.2.0
[0.1.0]: https://github.com/yutayouguan/astro-harness/releases/tag/v0.1.0
