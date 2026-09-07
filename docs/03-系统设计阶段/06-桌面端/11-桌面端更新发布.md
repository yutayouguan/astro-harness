# Astro 桌面端公开更新发布

Astro 的源码仓库可以保持私有。安装包、签名和 `latest.json` 发布到一个独立的公开
GitHub Releases 仓库，客户端只访问该公开仓库。

## 本机签名材料

- 私钥：`~/.tauri/astro-agent.key`
- 密码：macOS 钥匙串服务 `com.astroagent.desktop.updater-signing`
- 公钥：`apps/desktop/src-tauri/updater.pub`（可公开，已纳入版本控制）

私钥和密码不得提交到源码仓库，也不得上传为 Release 资产。

需要轮换密钥时，在 `apps/desktop` 下执行 `npx tauri signer generate
--write-keys ~/.tauri/astro-agent.key`，然后用新生成的 `.pub` 文件替换仓库中的
`apps/desktop/src-tauri/updater.pub`。发布新公钥版本后，旧版本客户端无法验证新密钥签名的
更新包，因此应先发布一次同时使用旧私钥、内置新公钥的过渡版本。

## GitHub 配置

1. 公开资产仓库固定为 [`yutayouguan/astro-agent-releases`](https://github.com/yutayouguan/astro-agent-releases)。该仓库只初始化 `main` 分支，不能推送 Astro 源码。
2. 创建只对公开发布仓库拥有 `Contents: Read and write` 权限的 fine-grained PAT，并保存为源码仓库 Secret `ASTRO_RELEASE_TOKEN`。
3. 将本机签名材料写入源码仓库 Secrets：

   ```bash
   gh auth login
   gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/astro-agent.key
   security find-generic-password \
     -a "$USER" \
     -s com.astroagent.desktop.updater-signing \
     -w | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
   ```

## 发布

推送 `v*` 标签或手动运行 `release-tauri`。工作流会：

1. 在私有源码仓库的 Runner 中构建应用。
2. 使用私钥签名更新包。
3. 将安装包、签名和 `latest.json` 上传到公开发布仓库。
4. 创建公开且非草稿的 Release；不会向发布仓库推送源码。

构建时客户端会写入以下更新地址：

```text
https://github.com/yutayouguan/astro-agent-releases/releases/latest/download/latest.json
```

本地开发构建未设置 `ASTRO_UPDATE_ENDPOINT` 时，关于页会明确显示更新功能尚未配置。
