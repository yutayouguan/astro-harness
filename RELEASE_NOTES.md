Astro Harness 是本地优先的多模态 AI Agent 桌面工作站：多模型对话、工具与 Skill、
内置浏览器、工作流与自动化。

### 本次发布

- **更名为 Astro Harness**（原 Astro Agent）：安装包、更新地址与仓库同步更名。
- **源码开源**：MIT OR Apache-2.0 双许可，仓库地址
  https://github.com/yutayouguan/astro-harness
- 安装包与 `latest.json` 直接发布在本仓库 Releases，应用内更新从这一版起可用。
- 提供商接入收窄为支持 Responses API 的厂商（OpenAI、DeepSeek、Azure OpenAI、
  OpenRouter、百炼、MiniMax，以及自定义 Provider）。
- 首次启动引导的密钥字段新增「获取密钥」入口。

### 已知限制

- macOS 与 Windows 安装包未做系统代码签名，首次打开请按下方提示放行。
- Windows 只提供 NSIS `setup.exe`；Linux 包在 Ubuntu 22.04 上构建，需要 glibc 2.35 及以上。
