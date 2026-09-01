# Azure OpenAI 官方文档镜像

本目录保存 Microsoft Learn 中 Azure OpenAI Responses API 相关中文文档的 Markdown 快照，便于离线检索和实现对照。

> - 抓取日期：2026-09-01
> - 内容来源：Microsoft Learn 返回的官方 Markdown（请求头 `Accept: text/markdown`）
> - 每篇文档的 front matter 均保留 `canonicalUrl`、`git_commit_id`、`ms.date` 和 `updated_at` 等上游元数据
> - 官方文档会持续更新；如本地内容与线上不一致，以原始链接为准

## 文档索引

| 本地文档 | 原始链接 |
| --- | --- |
| [Responses API](./responses.md) | [Microsoft Learn](https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/responses?tabs=python) |
| [Web 搜索](./web-search.md) | [Microsoft Learn](https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/web-search?tabs=python) |
| [Shell 工具](./shells.md) | [Microsoft Learn](https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/shells?tabs=python) |
| [工具搜索](./tool-search.md) | [Microsoft Learn](https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/tool-search?tabs=python) |
| [Skills](./skills.md) | [Microsoft Learn](https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/skills?tabs=python) |
| [提示缓存](./prompt-caching.md) | [Microsoft Learn](https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/prompt-caching) |
| [Embeddings](./embeddings.md) | [Microsoft Learn](https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/embeddings?tabs=python-new) |

用户提供的 Responses API 链接重复出现两次，本地仅保留一份 `responses.md`。

## 更新方法

在仓库根目录执行以下命令。命令会直接覆盖现有快照，因此更新前可先用 `git diff -- docs/azure/response_api` 检查当前改动。

```bash
set -euo pipefail

refresh_azure_doc() {
  local output="$1"
  local url="$2"
  local target="docs/azure/response_api/$output"
  local downloaded
  downloaded="$(mktemp)"
  curl --location --fail --silent --show-error \
    --header 'Accept: text/markdown' \
    "$url" \
    --output "$downloaded"
  perl -pe 's/\r$//; s/[ \t]+$//' "$downloaded" > "$target"
  rm "$downloaded"
}

refresh_azure_doc responses.md 'https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/responses?tabs=python'
refresh_azure_doc web-search.md 'https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/web-search?tabs=python'
refresh_azure_doc shells.md 'https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/shells?tabs=python'
refresh_azure_doc tool-search.md 'https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/tool-search?tabs=python'
refresh_azure_doc skills.md 'https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/skills?tabs=python'
refresh_azure_doc prompt-caching.md 'https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/prompt-caching'
refresh_azure_doc embeddings.md 'https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/embeddings?tabs=python-new'
```

更新后建议检查文件完整性和上游版本：

```bash
rg -n '^(title|canonicalUrl|git_commit_id|ms.date|updated_at):' docs/azure/response_api/*.md
git diff --stat -- docs/azure/response_api
git diff -- docs/azure/response_api
```
