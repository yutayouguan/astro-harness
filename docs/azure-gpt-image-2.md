# Azure AI Foundry `gpt-image-2` 使用说明

> 状态：已实现
>
> 更新：2026-09-01
>
> 适用范围：Azure AI Foundry OpenAI v1 图片生成端点

Astro 可以把 Azure AI Foundry 上的 `gpt-image-2` 作为 `image_gen` 图片提供商。当前 Azure 路径支持文生图，结果会保存到当前工作区的 `generated/images/` 目录。

实现设计见 [Azure AI Foundry `gpt-image-2` 接入设计](./superpowers/specs/2026-09-01-azure-gpt-image-2-design.md)。

## 1. 前置条件

- Azure AI Foundry 中已部署 `gpt-image-2`。
- 拥有该资源的 API Key。
- 使用 OpenAI v1 风格的 Foundry endpoint，必须以 `/openai/v1` 结尾。

Endpoint 格式：

```text
https://<resource>.services.ai.azure.com/openai/v1
```

不要填写完整的 `/images/generations` 路径，Astro 会自动追加。

## 2. Astro 配置

1. 打开「设置 → 模型提供商」。
2. 新建或编辑 Azure Provider。
3. 填写 Foundry endpoint 和 API Key。
4. 在「媒体」页将生图模型设为 `gpt-image-2`。
5. 保存并启用该 Provider。

API Key 也可由环境变量提供：

```bash
export AZURE_OPENAI_API_KEY="<your-api-key>"
# 或
export AZURE_API_KEY="<your-api-key>"
```

不要把真实 Key 写入仓库、聊天提示词或截图。

## 3. 生成图片

在聊天中启用 `image_gen` 工具后，可以直接提出生图要求，例如：

```text
生成一张秋日森林中的红狐摄影图，柔和自然光。
```

成功后工具返回：

```text
图片已生成：generated/images/<name>.png
provider=azure
model=gpt-image-2
```

生成的路径可继续传给 `video_gen` 的 `image`、`last_frame` 或 `reference_images`。

## 4. 当前能力与限制

| 能力                         | Azure `gpt-image-2`         |
| ---------------------------- | --------------------------- |
| 文本生图                     | 支持                        |
| `title` 文件名提示           | 支持                        |
| 输出数量                     | 固定 `n=1`                  |
| 输出尺寸                     | 固定 `1024x1024`            |
| 输出格式                     | PNG，压缩值 `100`           |
| 参考图 / 图片编辑            | 暂不支持                    |
| 自定义宽高比或分辨率         | 暂不支持                    |
| Search / thinking / 视频转图 | 仅 Google Interactions 支持 |

如果向 Azure 传入 `aspect_ratio`、`image_size`、`reference_images`、Search、thinking 或视频参数，Astro 会在发起请求前拒绝，避免参数被静默忽略后仍产生费用。

## 5. Provider 选择与 fallback

Astro 的图片候选顺序为：

```text
当前活动的图片 Provider
  → Google
  → OpenAI
  → Azure
  → MiniMax
```

- 独立生图命令会按顺序尝试所有可用候选。
- Agent `image_gen` 工具上下文当前只携带 primary 和 fallback 两个候选。
- 如果需要确保 Agent 使用 Azure，可将 Azure 设为当前活动 Provider，或关闭不需要的其他图片 Provider。

聊天模型和媒体生图模型是两个独立字段：Azure 聊天模型不应填成 `gpt-image-2`，应在「媒体」页单独配置它。

## 6. 实际 HTTP 契约

Astro 发起的请求与 Azure Foundry OpenAI v1 示例一致：

```http
POST https://<resource>.services.ai.azure.com/openai/v1/images/generations
Authorization: Bearer <api-key>
Content-Type: application/json
```

```json
{
  "model": "gpt-image-2",
  "prompt": "A photograph of a red fox in an autumn forest",
  "n": 1,
  "size": "1024x1024",
  "output_format": "png",
  "output_compression": 100
}
```

Astro 解码响应中的 `data[0].b64_json`；若兼容网关返回 `data[0].url`，则下载该 URL。

## 7. 排障

| 现象                    | 检查项                                                                 |
| ----------------------- | ---------------------------------------------------------------------- |
| 提示 endpoint 不正确    | 确认 base URL 以 `/openai/v1` 结尾，且未额外填写 `/images/generations` |
| HTTP 401 / 403          | 检查 Key、资源权限和 endpoint 是否属于同一 Azure 资源                  |
| HTTP 404                | 检查 `gpt-image-2` 部署名和 Foundry OpenAI v1 路径                     |
| HTTP 429                | 检查 Azure 配额、容量和重试频率                                        |
| 报“仅支持 prompt/title” | 删除 Azure 暂不支持的 Interactions 高级参数                            |
| Agent 未选到 Azure      | 确认 Azure 已启用且有 Key；再检查 primary/fallback 候选顺序            |

错误信息会包含 HTTP 状态、已脱敏 URL 和 Azure request ID（若有）。Astro 构造的诊断上下文不会插入 API Key，并会移除 URL query。

### Foundry v1 与传统 Azure deployment endpoint

| 类型                | Foundry OpenAI v1 图片                               | 传统 Azure deployment API             |
| ------------------- | ---------------------------------------------------- | ------------------------------------- |
| base URL            | `https://<resource>.services.ai.azure.com/openai/v1` | `https://<resource>.openai.azure.com` |
| 路径                | `/images/generations`                                | `/openai/deployments/<name>/...`      |
| 图片认证            | `Authorization: Bearer`                              | 通常为 `api-key`                      |
| 本次 Astro 图片实现 | 支持                                                 | 不支持                                |

不要把两种 endpoint 混用。
