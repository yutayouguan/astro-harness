# Azure AI Foundry `gpt-image-2` 接入设计

> 日期：2026-09-01
>
> 状态：已实现
>
> 对应提交：`66578f6f`

## 1. 背景

Astro 的 `image_gen` 已支持 Google Interactions、OpenAI Images 和 MiniMax。Azure AI Foundry 的 `gpt-image-2` 暴露 OpenAI v1 风格的图片端点：

```text
POST https://<resource>.services.ai.azure.com/openai/v1/images/generations
Authorization: Bearer <api-key>
```

它与 Astro 原有 Azure Chat/Responses 路径不同：图片端点不使用传统 deployment URL，认证也不复用 `AzureHeader` 的 `api-key` header。因此不能仅将 Azure 标记为普通 `OpenAi` 图片协议，必须显式区分路由和认证契约。

## 2. 目标

1. 将 Azure 加入 Astro 的图片生成能力表和设置页。
2. 按 Azure Foundry OpenAI v1 契约调用 `gpt-image-2`。
3. 保持 OpenAI 兼容路径的原有请求体不变。
4. 让 Agent `image_gen`、独立生图 RPC 和 provider registry 对 Azure 能力有一致认知。
5. 在请求前拒绝 Azure 尚未支持的高级参数，避免静默降级和无效计费。
6. 为 4xx/5xx 提供可排查但不泄露密钥的错误信息。

## 3. 非目标

- 本次不实现 Azure 参考图编辑、mask 或多轮图片编辑。
- 不将 Google Interactions 的 Search、thinking、video 参数映射到 Azure。
- 不开放 `n`、`size`、`output_format` 和 `output_compression` 为工具层参数。
- 不支持传统 `*.openai.azure.com/openai/deployments/...` 图片端点。
- 不改变 Azure Chat/Responses 的现有协议和认证行为。

## 4. 关键决策

| 决策     | 选择                                        | 原因                                                |
| -------- | ------------------------------------------- | --------------------------------------------------- |
| 图片路由 | 新增 `ImageGenMode::AzureOpenAiV1`          | 避免把 Azure 特殊契约混入普通 OpenAI 兼容路径       |
| 认证     | 图片请求单独使用 Bearer                     | 与 Foundry OpenAI v1 示例一致                       |
| endpoint | 要求 base URL 以 `/openai/v1` 结尾          | 早失败，防止误用传统 deployment endpoint            |
| 请求体   | 固定 PNG、1024 方形、`n=1`、compression 100 | 首版严格对齐已验证的官方调用契约                    |
| 高级参数 | HTTP 前拒绝                                 | 不让用户在参数无效时仍支付生成费用                  |
| 响应     | 优先 `b64_json`，兼容 `url`                 | 对齐 Azure 主契约，同时保留 OpenAI 兼容网关的灵活性 |
| 产物     | 继续写入 `generated/images/`                | 保持媒体工具与预览链路不变                          |

## 5. 总体架构

```text
ProvidersPanel
  └─ Azure endpoint / API Key / image_model=gpt-image-2
       ↓
resolve_image_gen_targets
  └─ active → Google → OpenAI → Azure → MiniMax
       ↓
ChatRequest image_gen primary + fallback
       ↓
ToolContext::image_gen_targets
       ↓
image_gen::generate_one_openai_compat
       ↓
providers::dispatch::generate_image
       ↓ profile.image_mode = AzureOpenAiV1
openai::image_http::azure_foundry_generate_image
       ↓
POST /openai/v1/images/generations
       ↓
data[].b64_json / data[].url
       ↓
workspace/generated/images/*.png
```

独立 Tauri `generate_image` 命令通过 gRPC `GenerateImage` 调用同一 `providers::dispatch::generate_image` 入口，但会遍历所有已解析候选；Agent 工具上下文只传递前两个候选。

## 6. 分层设计

### 6.1 Provider profile

Azure profile 声明：

```rust
supports_image_gen: true
image_mode: Some(ImageGenMode::AzureOpenAiV1)
default_image_model: "gpt-image-2"
```

`supports_image_gen()`、设置页能力和通用 `generate_image()` 都以 profile 为事实源。

### 6.2 Registry capability

`Azure` 的 `Capabilities::ImageGen` 为 `Capable<AzureImageModel>`，`Registry::register_azure()` 同时挂载 Chat 和 ImageGen。ImageGen model 使用 profile 的 `gpt-image-2` 默认值，不复用 Azure 聊天模型。

这保证类型能力、动态 registry 和 dispatch profile 三者一致。

### 6.3 Desktop 配置

`ProvidersPanel` 为 Azure 提供媒体默认值：

```text
image = gpt-image-2
video = ""
tts = ""
music = ""
vision = ""
```

`resolve_image_gen_targets()` 只接受已启用、存在 Key 且能解析图片默认模型的 Provider。活动 Provider 优先，防止 Azure 被 Google/OpenAI 挤出 Agent 的两个媒体凭据槽位。

### 6.4 Tool 参数边界

Google 继续使用 Interactions 高级参数。OpenAI/Azure/MiniMax 共用 prompt-only 工具分支。当以下任意参数存在时，工具在 Provider HTTP 请求前返回错误：

- `aspect_ratio`
- `image_size`
- `reference_images`
- `previous_interaction_id`
- `google_search` / `image_search`
- `thinking_level`
- `video` / `video_uri`

`prompt` 与用于本地文件命名的 `title` 可用。

## 7. HTTP 契约

### 7.1 URL 与认证

```text
base_url = trim_trailing_slash(config.base_url)
assert base_url.ends_with("/openai/v1")
url = base_url + "/images/generations"
Authorization = "Bearer " + api_key
Content-Type = "application/json"
```

Azure 图片路径不调用 `ProviderExt::auth_headers()`，因为 Azure Chat 使用的 `api-key` 协议不适用于此 Foundry v1 图片端点。

### 7.2 请求

```json
{
  "model": "gpt-image-2",
  "prompt": "<prompt>",
  "n": 1,
  "size": "1024x1024",
  "output_format": "png",
  "output_compression": 100
}
```

`output_format` 和 `output_compression` 只在 `AzureOpenAiV1` flavor 中添加。普通 OpenAI-compatible 请求体保持 `model/prompt/n/size`，防止兼容网关回归。

### 7.3 响应

1. 读取完整响应 bytes，保留非 JSON 错误体。
2. 非 2xx 时优先提取 `error.message`。
3. 2xx 时解析 `data` 数组。
4. `b64_json` 按 Base64 解码为 PNG bytes。
5. 若返回 `url`，使用共享 HTTP client 下载。
6. 没有任何图片数据时返回明确错误。

## 8. 错误与安全

错误格式：

```text
Azure Foundry 图片生成 HTTP <status> (<sanitized-url>, request_id=<id>): <message>
```

安全约束：

- Astro 构造的错误上下文不插入 API Key。
- 错误中的 URL 移除 query 和 fragment。
- 兼容 `x-request-id`、`apim-request-id` 和 `x-ms-request-id`。
- 非结构化错误体最多保留 4096 个字符。
- 无 Key 和 endpoint 协议错误在发起网络请求前失败。

## 9. Fallback 语义

Agent `image_gen` 按 primary、fallback 顺序尝试：

```text
primary success  → 返回产物
primary failure  → 尝试 fallback
fallback failure → 聚合 provider/model/error
```

当前没有按 HTTP 状态分类是否应 fallback；包括参数错误在内的任何失败都会继续尝试下一候选。这是现有工具语义，本设计不改变它。

## 10. 兼容性

- OpenAI 图片请求体不增加 Azure-only 字段。
- Google 继续走 Interactions，高级参数契约不变。
- MiniMax 继续走原生 T2I 路径。
- 旧 `providers.json` 没有 Azure `image_model` 时，通过 profile 回退 `gpt-image-2`。
- 聊天模型与图片模型分开解析。

## 11. 测试策略

### Provider 单元测试

- Azure profile 声明 `supports_image_gen` 和 `AzureOpenAiV1`。
- Azure registry 同时挂载 Chat 与 ImageGen。
- 精确断言 URL、Bearer header 和 Azure 请求 JSON。
- 断言普通 OpenAI 请求不含 Azure-only 字段。
- 断言结构化错误提取和 URL query 脱敏。
- 断言传统 Azure endpoint 在网络请求前被拒绝。

### Tool 测试

- `aspect_ratio` 等高级字段被识别。
- 高级字段不会到达 OpenAI/Azure HTTP 路径。
- 产物目录仍为 `generated/images/`。

真实 Azure 调用不作为必跑 CI 测试，避免依赖秘密、配额和区域容量。

## 12. 后续演进

1. 将媒体 Provider 选择从“活动聊天 Provider”中独立，不再受两个凭据槽位限制。
2. 在确认 Azure `gpt-image-2` 参数契约后，将 `size`、`n`、quality 等提升为通用工具参数。
3. 根据错误类型区分可重试失败与确定性参数错误。
4. 对大尺寸 Base64 响应增加内存上限和流式落盘策略。
