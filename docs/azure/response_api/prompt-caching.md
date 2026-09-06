---
layout: Conceptual
title: 在 Microsoft Foundry 模型中使用 Azure OpenAI 进行提示缓存 - Microsoft Foundry | Microsoft Learn
canonicalUrl: https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/prompt-caching
schema: Conceptual
ai-usage: ai-assisted
author: alvinashcraft
breadcrumb_path: ../../../breadcrumb/azure-ai/toc.json
depot_name: Learn.azure-ai
description: 了解如何将提示缓存与 Azure OpenAI 配合使用
feedback_help_link_type: get-help-at-qna
feedback_help_link_url: https://learn.microsoft.com/answers/tags/133/azure
feedback_product_url: https://feedback.azure.com/d365community/forum/79b1327d-d925-ec11-b6e6-000d3a4f06a4
feedback_system: Standard
git_commit_id: 669a8a54792018b1b0cb9063bf002d5e64457e9f
gitcommit: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/669a8a54792018b1b0cb9063bf002d5e64457e9f/articles/foundry/openai/how-to/prompt-caching.md
learn_banner_products:
- azure
locale: zh-cn
manager: mcleans
ms.author: aashcraft
ms.collection: ce-skilling-ai-copilot
ms.custom:
- classic-and-new
- doc-kit-assisted
ms.date: 2026-08-11T00:00:00.0000000Z
ms.service: microsoft-foundry
ms.subservice: foundry-openai
ms.suite: office
ms.topic: how-to
ms.update-cycle: 90-days
original_content_git_url: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/live/articles/foundry/openai/how-to/prompt-caching.md
permissioned-type: public
recommendation_types:
- Training
- Certification
recommendations: false
services: cognitive-services
site_name: Docs
uhfHeaderId: azure-ai-foundry
updated_at: 2026-08-14T22:19:00.0000000Z
ms.translationtype: MT
ms.contentlocale: zh-cn
loc_version: 2026-08-12T16:04:16.1268685Z
loc_source_id: Github-845157915#live
loc_file_id: Github-845157915.live.Learn.azure-ai.articles/foundry/openai/how-to/prompt-caching.md
page_type: conceptual
toc_rel: ../../toc.json
word_count: 3150
asset_id: foundry/openai/how-to/prompt-caching
item_type: Content
cmProducts:
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/8a6e4dad-7050-4ce7-83f9-eb4123577a54
- https://authoring-docs-microsoft.poolparty.biz/devrel/68ec7f3a-2bc6-459f-b959-19beb729907d
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/de19c5b8-e208-412e-9238-db3f631dea5b
spProducts:
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/0a5fc323-00ce-4c20-9095-41948f54c83f
- https://authoring-docs-microsoft.poolparty.biz/devrel/90370425-aca4-4a39-9533-d52e5e002a5d
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/ea7bf5d6-7154-4ba9-8ebc-59117ccacd49
platformId: 052b3d73-75ff-42ad-3168-0a449f457af5
---

# 在 Microsoft Foundry 模型中使用 Azure OpenAI 进行提示缓存 - Microsoft Foundry | Microsoft Learn

提示缓存可减少在提示开始时具有相同内容的较长提示的总体请求延迟和成本。 在此上下文中， *“提示”* 是指作为聊天完成或响应创建请求的一部分发送到模型的输入。 服务不会反复重新处理相同的输入令牌，而是保留已处理的输入令牌计算的临时缓存，以提高整体性能。 提示缓存不会影响模型响应中返回的输出内容，但延迟和成本会降低。

对于受支持的模型，缓存读取在标准部署类型中按[输入令牌价格折扣](https://azure.microsoft.com/pricing/details/cognitive-services/openai-service/)计费，在预配部署类型中可享受[输入令牌最高 100% 折扣](/zh-cn/azure/ai-foundry/openai/concepts/provisioned-throughput)。 提示缓存的定价在这两种保留策略下是相同的。

Important

GPT-5.6 系列之前的模型不收取写入缓存的额外费用。 在 GPT-5.6 模型和更高版本的模型系列上，缓存写入除了折扣缓存读取之外，还会产生费用。 为了使成本保持可预测性，请合理组织提示词，使重复使用的内容在不同请求之间保持完全一致，从而更倾向于缓存读取而非缓存写入。 有关当前费率，请参阅 [Azure OpenAI 定价页](https://azure.microsoft.com/pricing/details/cognitive-services/openai-service/)。

## 使用提示缓存密钥提高缓存命中率

在 GPT-5.6 模型和更高版本的模型系列上，设置 `prompt_cache_key` 参数，并为共享长且常见的提示前缀的请求重复使用相同的密钥。 此参数改进了相关请求的缓存匹配。 不需要使用特定的 API 版本 `prompt_cache_key`。 对于新的集成，请使用 [v1 API](../api-version-lifecycle)。

如果对相同前缀和 `prompt_cache_key` 组合的请求每分钟超过大约 15 个请求，则某些请求可能会错过缓存。 对于请求量较大的工作负载，请将请求分散到多个密钥，同时保持每个密钥与其共享的提示词前缀之间的稳定映射关系。

## 配置提示缓存断点

在 GPT-5.6 模型和更高版本的模型系列上，使用显式缓存断点来标记可重用提示前缀的末尾。 响应 API 和聊天完成 API 都支持断点。 Azure OpenAI 使用与 OpenAI API 相同的请求结构，但设置为`model`Azure模型部署名称。 断点后的内容可以更改，而不会使缓存的前缀失效。

标准即用即付部署支持提示缓存断点。 [预配的吞吐量托管（PTU-M）](../concepts/provisioned-throughput) 部署不支持提示缓存断点。

使用 `prompt_cache_options.mode`以下命令设置请求范围的缓存策略：

| **模式** | **行为** |
| --- | --- |
| `implicit` | 默认值。 Azure OpenAI 在最新消息上放置断点，还使用你提供的任何显式断点。 |
| `explicit` | Azure OpenAI 仅对缓存读取和写入使用显式断点。 如果请求不包含显式断点，则不会使用提示缓存或产生缓存写入费用。 |

`prompt_cache_options.ttl`设置为`30m`配置请求中所有断点的最小缓存生存期。 该值 `30m` 为默认值和唯一支持的值。 此设置不会选择内存中或扩展保留策略。

添加到 `prompt_cache_breakpoint: { "mode": "explicit" }` 受支持的提示内容块。 断点在可重用前缀中包含块和所有提示内容。

- 响应 API 支持断点和`input_text``input_image``input_file`块。
- 聊天完成 API 支持断点、`text``image_url``input_audio`块和`file`块。

### 断点限制

- 每个请求最多可以创建四个新的缓存写入。
- 在 `implicit` 模式下，最新消息上的断点使用一个写入槽，因此请求最多可以写入最新的三个显式断点。
- 在 `explicit` 模式下，请求最多可以写入最新的四个显式断点。
- 早期会话轮次的断点是只读的。 它们可以匹配缓存，但请求不会再次写入。
- 对于缓存读取，Azure OpenAI 最多考虑聊天中的 50 个断点。

在以下示例中，通过显式断点呈现的前缀必须至少包含 1,024 个可缓存的令牌。

以下响应 API 请求使用默认 `implicit` 模式，并在稳定引用文件后添加显式断点：

```json
{
  "model": "<your-gpt-5.6-deployment-name>",
  "prompt_cache_key": "tenant:contoso:product-manual-v2",
  "input": [
    {
      "type": "message",
      "role": "user",
      "content": [
        {
          "type": "input_file",
          "file_id": "<product-manual-file-id>",
          "prompt_cache_breakpoint": { "mode": "explicit" }
        },
        {
          "type": "input_text",
          "text": "Summarize the troubleshooting procedures."
        }
      ]
    }
  ]
}
```

以下聊天完成 API 请求使用 `explicit` 模式并标记可重用系统消息的末尾：

```json
{
  "model": "<your-gpt-5.6-deployment-name>",
  "prompt_cache_key": "tenant:contoso:support-policy-v2",
  "prompt_cache_options": { "mode": "explicit", "ttl": "30m" },
  "messages": [
    {
      "role": "system",
      "content": [{
        "type": "text",
        "text": "<at least 1,024 tokens of reusable instructions>",
        "prompt_cache_breakpoint": { "mode": "explicit" }
      }]
    },
    {
      "role": "user",
      "content": "<variable user input>"
    }
  ]
}
```

注意

GPT-5.6 系列之前的模型不支持 `prompt_cache_options` 或 `prompt_cache_breakpoint`。 包含这些参数的请求将返回错误 `400` 。 继续对这些模型使用自动提示缓存。

## 快速缓存保留

提示缓存具有两个具有不同语义的控件：

- 在 GPT-5.6 模型和更高版本的模型系列上， `prompt_cache_options.ttl` 设置最小缓存生存期。 它不选择存储策略或最大保留期。
- 对于早期模型， `prompt_cache_retention` 选择最大保留策略。 在 GPT-5.6 模型和更高版本的模型系列上，此字段不适用且已弃用。

在 GPT-5.6 模型和更高版本的模型系列上，用于 `prompt_cache_options.ttl` 设置请求写入的所有断点的最小生存期。 唯一支持的值也是 `30m`默认值。 缓存的前缀仍有资格重复使用至少 30 分钟，但服务可能会保留更长时间。

对于 GPT-5.6 系列之前的模型，请在“响应”或“聊天完成”请求上设置 `prompt_cache_retention` 。 提示缓存可以采用内存内或延长保留策略。 如果可用，扩展提示缓存旨在将缓存保留更长时间，以便后续请求更有可能与缓存匹配。 这两个策略的提示缓存定价相同。

### 内存中提示缓存保留

系统通常会在非活动状态的 5 到 10 分钟内清除缓存，并在缓存上次使用后的一小时内始终将其删除。 系统不会在Azure订阅之间共享提示缓存。

所有 GPT-4o 及更新版本的 Azure OpenAI 模型都支持内存中提示缓存保留功能。 着适用于具有聊天补全、补全、响应或实时操作的模型。 对于没有这些操作的模型，此功能不可用。

### 扩展的提示缓存保留期

延长的提示缓存保留期使缓存的前缀保持活动时间更长，最长为 24 小时。 当内存已满时，扩展提示缓存的工作原理是将键/值张量卸载到 GPU 本地存储，这大大增加了可用于缓存的存储容量。

扩展的提示缓存保留期适用于以下模型：

- `gpt-5.5`
- `gpt-5.4`
- `gpt-5.3-codex`
- `gpt-5.2`
- `gpt-5.1-codex-max`
- `gpt-5.1`
- `gpt-5.1-codex`
- `gpt-5.1-codex-mini`
- `gpt-5.1-chat`
- `gpt-5`
- `gpt-5-codex`
- `gpt-4.1`

### 按请求配置

对于 `gpt-5.4` 旧模型，如果未指定保留策略，则默认值为 `in_memory`。 允许的值为 `in_memory` 和 `24h`。 对于 `gpt-5.5`，默认启用延长保留期。

```json
{
  "model": "<your-gpt-5.4-deployment-name>",
  "input": "Your prompt goes here...",
  "prompt_cache_retention": "24h"
}
```

## 入门指南

若要利用提示缓存，请求必须满足以下两项要求：

- 长度至少为 1,024 个令牌。
- 提示符中的前 1,024 个令牌必须相同。

如果提示中的令牌计算与提示缓存的当前内容能够成功匹配，这种情况便称为缓存命中。 缓存命中将在聊天补全响应中的 [`cached_tokens`](/zh-cn/rest/api/microsoft-foundry/azureopenai/chat?view=rest-microsoft-foundry-2025-04-01-preview&amp;preserve-view=true) 下显示为 [`prompt_tokens_details`](/zh-cn/rest/api/microsoft-foundry/azureopenai/chat?view=rest-microsoft-foundry-2025-04-01-preview&amp;preserve-view=true)。

在 GPT-5.6 模型和更高版本的模型系列上，标准即用即付部署报表缓存读取和 `cached_tokens` 缓存写入 `cache_write_tokens`。 以下摘录显示了聊天完成响应中的这些字段。 JSON 属性顺序不重要，可能有所不同。

```json
{
  "usage": {
    "prompt_tokens": 1566,
    "completion_tokens": 1518,
    "total_tokens": 3084,
    "prompt_tokens_details": {
      "audio_tokens": null,
      "cached_tokens": 1408,
      "cache_write_tokens": 0
    },
    "completion_tokens_details": {
      "audio_tokens": null,
      "reasoning_tokens": 576
    }
  }
}
```

在 GPT-5.5 和更早的模型中，在前 1,024 个令牌以 128 个令牌增量出现后缓存命中。 此舍入不适用于 GPT-5.6 模型和更高型号系列。

前 1,024 个令牌中的单个字符差异会导致缓存缺失，其特征是 `cached_tokens` 值为 0。 默认情况下，支持模型的提示缓存处于启用状态。

## 最佳做法

- 将稳定或重复的内容放在提示的开头，并在末尾放置动态内容。 仅保留对话上下文追加。
- 对共享前缀的请求重复使用一致 `prompt_cache_key` 。 对于大容量工作负荷，请跨键对流量进行分区，同时保持每个键与其前缀之间的稳定映射。
- 使用 GPT-5.6 模型和更高版本的模型系列进行标准即用即付部署时，在稳定内容后放置显式断点。 `explicit`如果只希望提供的断点符合缓存读取和写入条件，请使用模式。
- 使用 `cached_tokens`.. 监视缓存读取。 使用 GPT-5.6 模型和更高版本的模型系列进行标准即用即付部署时，还可以监视缓存写入， `cache_write_tokens` 并将写入量与以后的缓存读取进行比较。
- 保持具有相同前缀的请求的稳定流，以提高缓存重用率。

## 常见问题

以下答案阐明了支持的缓存内容、成本、部署类型和数据驻留。

### 什么是缓存的？

对 o1 系列模型的功能支持因模型而异。 有关详细信息，请参阅专用 [推理模型指南](reasoning)。

提示缓存支持：

| **支持的缓存** | **描述** |
| --- | --- |
| **消息** | 完整的消息数组：系统、开发人员、用户和助理内容 |
| **图像** | 用户消息中包含的图像可以是链接形式或 base64 编码的数据。 必须在所有请求中设置相同的详细信息参数。 |
| **工具使用** | 消息数组和工具定义。 |
| **结构化输出** | 结构化输出架构作为前缀追加到系统消息中。 |

为了提高缓存命中的可能性，请构建请求，以便在消息数组的开头出现重复内容。

### 是否可以禁用提示缓存？

使用 GPT-5.6 模型和更高版本的模型系列进行标准即用即付部署，设置为`prompt_cache_options.mode``explicit`不添加任何显式断点。 请求不使用提示缓存或产生缓存写入费用。 早期模型和 PTU-M 部署不支持此选项;默认情况下，提示缓存将保持启用状态。

### 是否为写入缓存支付额外的费用？

在 GPT-5.6 系列之前的模型上，写入缓存无需额外付费。 在 GPT-5.6 模型和更高版本的模型系列上，缓存写入除了折扣缓存读取之外，还会产生费用。 若要查看当前费率，请转到 [Azure OpenAI 定价页](https://azure.microsoft.com/pricing/details/cognitive-services/openai-service/)。

### 提示缓存断点是否适用于 PTU-M？

在 GPT-5.6 模型和更高版本的模型系列上，标准即用即付部署支持提示缓存断点并公开 `cache_write_tokens`。 [预配的吞吐量托管（PTU-M）](../concepts/provisioned-throughput) 部署继续支持提示缓存，但它们不支持提示缓存断点或公开 `cache_write_tokens`。

### 提示缓存机制是否适用于数据驻留？

内存中提示缓存与所有数据驻留区域兼容。 扩展的提示缓存暂时将数据存储在 GPU 计算机上。 数据保留在数据区域标准和数据区域预配部署类型的数据区域边界内，以及区域标准和区域预配部署类型的区域边界内。