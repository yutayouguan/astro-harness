# 更新日志

本项目从 0.1.0 起以闭源二进制形式分发，源码仓库保持私有；安装包、更新签名与
`latest.json` 发布在公开资产仓库 `yutayouguan/astro-agent-releases`。

## [Unreleased]

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

[Unreleased]: https://github.com/yutayouguan/astro-agent-releases/releases
[0.1.0]: https://github.com/yutayouguan/astro-agent-releases/releases/tag/v0.1.0
