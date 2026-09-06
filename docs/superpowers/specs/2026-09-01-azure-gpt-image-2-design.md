# Azure OpenAI v1 Responses 与 `gpt-image-2` 接入设计

> 日期：2026-09-01
>
> 状态：已实现

## 1. 背景

Azure 已为 Responses 和 Images 提供 OpenAI v1 兼容入口：

```text
https://<resource>.services.ai.azure.com/openai/v1
https://<resource>.openai.azure.com/openai/v1
```

两种主机名都可以调用 `/responses` 与 `/images/generations`，部署名通过请求体 `model` 传入，认证使用 `Authorization: Bearer`。新配置默认采用 `services.ai.azure.com`，同时保留 `openai.azure.com` 兼容。

## 2. 目标

1. Azure Agent 聊天默认走 Responses API，生图走 Images API。
2. 两条路径共用同一 endpoint、Key 和 Bearer 认证。
3. 保留聊天模型与生图模型的独立配置。
4. 让 Agent 工具、独立生图 RPC 和 Workflow 共用同一 Provider 配置与能力路由。
5. 允许独立设置“默认生图 Provider”，不影响默认聊天 Provider。
6. 对错误、超时和大响应设置可诊断且不泄密的边界。

## 3. 非目标

- 不支持旧式 `/openai/deployments/<name>/...?...api-version=...` 请求形式。
- 不实现 Azure 图片编辑、mask 或参考图输入。
- 不将 Google Interactions 的 Search、thinking、video 参数映射到 Azure。
- 不在仓库、Workflow JSON 或运行日志中存储明文 API Key。

## 4. 关键决策

| 决策 | 选择 | 理由 |
| --- | --- | --- |
| 统一基址 | `{resource}/openai/v1` | 与 OpenAI SDK `base_url` 契约一致 |
| 默认主机 | `services.ai.azure.com` | 新配置使用当前 Azure AI Services 入口 |
| 兼容主机 | `openai.azure.com` | 保留已有可用资源，根地址可自动补齐 v1 路径 |
| 认证 | Bearer | Responses 与 Images 使用一致认证 |
| 聊天请求 | `POST /responses` | Agent 保持原生 Responses 事件语义 |
| 图片请求 | `POST /images/generations` | 与 `OpenAI.images.generate` 一致 |
| 模型 | 聊天与生图分字段 | 同一 Azure Provider 可同时使用 `gpt-5.6-sol` 和 `gpt-image-2` |
| 媒体默认 | `active_image_provider_id` | 不为生图切换默认聊天模型 |
| 工作流密钥 | 运行时注入 | 使用 keyring/环境变量，不写入节点配置 |

## 5. 总体架构

```text
ProvidersPanel
  ├─ endpoint + API Key
  ├─ chat model = gpt-5.6-sol
  ├─ image model = gpt-image-2
  └─ active_image_provider_id
       ↓
resolve_image_gen_targets
       ↓
Agent image_gen / GenerateImage RPC / Workflow ImageGen
       ↓
providers::dispatch::generate_image_with_options
       ↓ profile.image_mode = AzureOpenAiV1
openai::image_http::azure_foundry_generate_image_with_config
       ↓
POST {base}/images/generations
       ↓
data[].b64_json / data[].url
       ↓
generated/images/* 或 Workflow artifacts/*
```

Responses 路径由同一 Azure profile 注册 `OpenAIResponsesModel`：

```text
AgentLoop → agent_responses_stream → POST {base}/responses
```

## 6. Provider 契约

Azure profile 的核心字段：

```rust
api_mode: ApiMode::Responses
auth: AuthKind::Bearer
azure_deployment_style: false
default_base_url: "https://YOUR_RESOURCE.services.ai.azure.com/openai/v1"
default_model: "gpt-5.6-sol"
supports_image_gen: true
image_mode: Some(ImageGenMode::AzureOpenAiV1)
default_image_model: "gpt-image-2"
```

`azure_openai_v1_base()` 接受两种主机名的资源根地址、`/openai` 或 `/openai/v1` 形式，并输出唯一 v1 基址。请求体必须保留 `model`，其值是 Azure 部署名。

## 7. 图片请求与响应

默认请求：

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

`ImageGenConfig` 统一承载 `model`、`width/height`、`n`、`output_format`、`output_compression`、`quality`、`background` 和额外参数。尺寸必须成对出现，`n` 限制为 `1..=10`，格式限定为 PNG/JPEG/WebP。

Agent `image_gen` 目前仍使用默认 `n=1` 和 `1024x1024`；独立 RPC 可传递数量和尺寸，Workflow 还可选择输出格式。

响应优先解码 `data[].b64_json`，也兼容 `data[].url`。单图解码后上限为 32 MiB，JSON 响应上限为 128 MiB，共享 HTTP client 使用 30 秒连接超时和 180 秒总超时。

## 8. Workflow 与密钥边界

Workflow 节点只保存 Provider 记录 ID 和可选模型覆盖。手动运行时，Desktop 从 keyring/环境变量解析密钥，构造只存在于内存的 `RuntimeProviderConfig`；headless 运行可从已知环境变量获取密钥。该结构的 `Debug` 实现始终将 Key 显示为 `[REDACTED]`。

子工作流继承同一份运行时 Provider 映射，不会把凭据复制到 Workflow JSON 或节点输出。

## 9. 错误与安全

- 错误包含 HTTP 状态、去除 query/fragment 的 URL 和 Azure request ID。
- 结构化错误优先读取 `error.message`，非结构化错误最多保留 4096 字符。
- URL 回退下载只允许 HTTP(S)，拒绝用户信息、localhost 和显式私有 IP。
- API Key 不出现在 URL、错误上下文、Workflow 日志或 `Debug` 输出中。
- 参考图、图生图和 Google 专用高级参数在 Azure 请求前明确拒绝，不静默忽略。

## 10. Fallback 语义

Provider 选择优先级：

```text
active_image_provider_id
  → active_provider_id（未设置独立生图默认时）
  → Google → OpenAI → Azure → MiniMax
```

独立生图命令会按顺序尝试所有候选。Agent `image_gen` 工具上下文仍只携带 primary 和 fallback 两个候选，但独立生图默认保证指定的 Azure Provider 优先进入该链。

## 11. 测试策略

- URL 规范化覆盖两种 Azure 主机名与资源根地址。
- Azure profile 断言 Responses、Bearer、`gpt-5.6-sol` 和 `gpt-image-2`。
- 图片请求断言 URL、Bearer header、模型、尺寸、数量和输出参数。
- 断言错误提取、URL 脱敏、输出格式校验与本地 URL 拒绝。
- Provider 状态断言旧 JSON 兼容与无效生图默认自动清理。
- 真实 Azure 调用不进入必跑 CI，避免依赖秘密、配额和区域容量。
