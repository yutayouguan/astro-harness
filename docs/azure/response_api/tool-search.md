---
layout: Conceptual
title: 将工具搜索与 Azure OpenAI 响应 API 配合使用 - Microsoft Foundry | Microsoft Learn
canonicalUrl: https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/tool-search
schema: Conceptual
ai-usage: ai-assisted
author: alvinashcraft
breadcrumb_path: ../../../breadcrumb/azure-ai/toc.json
depot_name: Learn.azure-ai
description: 了解如何通过 Azure OpenAI 响应 API 使用工具搜索来延迟工具定义、减少上下文使用情况并保留提示缓存。
feedback_help_link_type: get-help-at-qna
feedback_help_link_url: https://learn.microsoft.com/answers/tags/133/azure
feedback_product_url: https://feedback.azure.com/d365community/forum/79b1327d-d925-ec11-b6e6-000d3a4f06a4
feedback_system: Standard
git_commit_id: d90f4f4c3be0233546c46bbff3555522b1a43430
gitcommit: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/d90f4f4c3be0233546c46bbff3555522b1a43430/articles/foundry/openai/how-to/tool-search.md
learn_banner_products:
- azure
locale: zh-cn
manager: mcleans
ms.author: aashcraft
ms.collection: ce-skilling-ai-copilot
ms.custom:
- doc-kit-assisted
ms.date: 2026-07-15T00:00:00.0000000Z
ms.service: microsoft-foundry
ms.subservice: foundry-openai
ms.suite: office
ms.topic: how-to
ms.update-cycle: 90-days
original_content_git_url: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/live/articles/foundry/openai/how-to/tool-search.md
permissioned-type: public
recommendation_types:
- Training
- Certification
recommendations: true
site_name: Docs
uhfHeaderId: azure-ai-foundry
updated_at: 2026-08-13T08:13:00.0000000Z
ms.translationtype: MT
ms.contentlocale: zh-cn
loc_version: 2026-07-23T22:20:00.8476724Z
loc_source_id: Github-845157915#live
loc_file_id: Github-845157915.live.Learn.azure-ai.articles/foundry/openai/how-to/tool-search.md
page_type: conceptual
toc_rel: ../../toc.json
word_count: 3540
asset_id: foundry/openai/how-to/tool-search
item_type: Content
platformId: 0fd100d5-bef3-7bed-4116-82e30eee039f
---

# 将工具搜索与 Azure OpenAI 响应 API 配合使用 - Microsoft Foundry | Microsoft Learn

工具搜索允许模型根据需要动态搜索和加载工具定义，因此无需提前加载每个工具定义。 此方法可以减少令牌使用情况和成本。 工具搜索还会通过在上下文窗口末尾注入新发现的工具来保留模型的缓存。 通过 Azure OpenAI Responses API 中的 `tool_search` 工具访问工具搜索。

注释

`gpt-5.4` 及后续模型支持工具搜索。 在以下示例中，将 `gpt-5.5` 替换为你的模型部署名称。

响应 API 中的工具搜索不同于 [Foundry 代理服务工具箱中的工具搜索](../../agents/how-to/tools/tool-search)。 Responses API 的工具搜索功能会延后处理你在请求中声明的工具定义。 工具箱中的工具搜索功能可查找在带版本的 Foundry 工具箱中配置的工具。

## 先决条件

- 支持工具搜索的 Azure OpenAI 模型部署。
- 身份验证方法：

    - API 密钥，或
    - Microsoft Entra ID（建议）。
- 为语言安装客户端库：

    - **Python**：

        ```bash
        pip install openai azure-identity
        ```
    - **JavaScript**：

        ```bash
        npm install openai @azure/identity
        ```
- 对于 REST 请求，请设置为 `AZURE_OPENAI_API_KEY` API 密钥身份验证或`AZURE_OPENAI_AUTH_TOKEN`Microsoft Entra ID身份验证。

## 了解工具搜索的工作原理

若要激活工具搜索，请执行以下操作：

1. 将`tool_search`添加到`tools`数组中。
2. 用 `defer_loading: true` 标记您想要暂缓处理的工具。

可以延迟单个函数、命名空间内的函数和 MCP 服务器。 延迟工具时，模型在确定需要该工具之前不会将其完整定义加载到上下文中。 然后，模型使用工具搜索在调用相关工具之前加载相关工具。

## 尽可能使用命名空间

您可以将工具搜索与延迟函数、命名空间或 MCP 服务器结合使用。 尽可能使用命名空间或 MCP 服务器。 模型主要经过训练来搜索这些表面，而且它们通常能节省更多令牌。

对于命名空间， `defer_loading` 适用于命名空间内的函数，不适用于命名空间对象本身。

在请求开始时，模型仍会看到每个可搜索图面的名称和说明。 对于命名空间或 MCP 服务器，模型仅看到命名空间或服务器名称和说明。 在工具搜索加载它们之前，它看不到各个函数定义。 对于单独延迟处理的函数，模型仍然会看到函数名称和描述，因此工具搜索主要延迟参数模式。

为了最大限度地节省令牌，请将延迟函数归类到命名空间或 MCP 服务器中。 为每个界面提供清晰的高层次概述，以概括其内容。 然后，模型可以搜索并仅加载相关函数。

小窍门

将每个命名空间保留为少于 10 个函数，以提高令牌效率和模型性能。

以下工具配置定义具有一个延迟函数的命名空间：

```json
{
  "tools": [
    {
      "type": "namespace",
      "name": "crm",
      "description": "CRM tools for customer lookup and order management.",
      "tools": [
        {
          "type": "function",
          "name": "list_open_orders",
          "description": "List open orders for a customer ID.",
          "defer_loading": true,
          "parameters": {
            "type": "object",
            "properties": {
              "customer_id": { "type": "string" }
            },
            "required": ["customer_id"],
            "additionalProperties": false
          }
        }
      ]
    },
    {
      "type": "tool_search"
    }
  ]
}
```

命名空间可以混用延迟工具和非延迟工具。 不带有 `defer_loading: true` 的工具可立即调用。 同一命名空间中的延迟工具会通过工具搜索来加载。

## 选择工具搜索类型

工具搜索支持两种执行类型：

- **托管工具搜索**：Azure OpenAI 搜索请求中声明的延迟工具，并在相同的响应中返回加载的子集。
- **客户端执行的工具搜索**：模型输出一个 `tool_search_call`。 您的应用程序执行查找操作并返回匹配的`tool_search_output`。

如果您在创建请求时已经知道候选工具，请先从托管工具搜索开始。 当工具发现依赖于应用程序控制的项目状态、租户状态或其他系统时，请使用客户端执行的工具搜索。

## 使用托管工具搜索

当知道模型可以搜索的函数、命名空间或 MCP 服务器的完整清单时，托管工具搜索是最简单的选项。 声明清单、添加`{"type": "tool_search"}`和让Azure OpenAI 确定要加载的工具。

### 使用 Microsoft Entra ID 进行身份验证

以下示例定义一个命名空间，该命名空间包含立即可用的函数和延迟函数。 提示需要延迟函数，因此模型在创建函数调用之前先搜索命名空间。

# [Python](#tab/python)
```python
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/"
token_provider = get_bearer_token_provider(
    DefaultAzureCredential(), "https://ai.azure.com/.default"
)

openai = OpenAI(base_url=endpoint, api_key=token_provider)

crm_namespace = {
    "type": "namespace",
    "name": "crm",
    "description": "CRM tools for customer lookup and order management.",
    "tools": [
        {
            "type": "function",
            "name": "get_customer_profile",
            "description": "Fetch a customer profile by customer ID.",
            "parameters": {
                "type": "object",
                "properties": {"customer_id": {"type": "string"}},
                "required": ["customer_id"],
                "additionalProperties": False,
            },
        },
        {
            "type": "function",
            "name": "list_open_orders",
            "description": "List open orders for a customer ID.",
            "defer_loading": True,
            "parameters": {
                "type": "object",
                "properties": {"customer_id": {"type": "string"}},
                "required": ["customer_id"],
                "additionalProperties": False,
            },
        },
    ],
}

response = openai.responses.create(
    model="gpt-5.5",
    input="List open orders for customer CUST-12345.",
    tools=[crm_namespace, {"type": "tool_search"}],
    parallel_tool_calls=False,
)

print(response.output)
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";
import {
  DefaultAzureCredential,
  getBearerTokenProvider,
} from "@azure/identity";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";
const tokenProvider = getBearerTokenProvider(
  new DefaultAzureCredential(),
  "https://ai.azure.com/.default"
);

const openai = new OpenAI({
  baseURL: endpoint,
  apiKey: await tokenProvider(),
});

const crmNamespace = {
  type: "namespace",
  name: "crm",
  description: "CRM tools for customer lookup and order management.",
  tools: [
    {
      type: "function",
      name: "get_customer_profile",
      description: "Fetch a customer profile by customer ID.",
      parameters: {
        type: "object",
        properties: { customer_id: { type: "string" } },
        required: ["customer_id"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "list_open_orders",
      description: "List open orders for a customer ID.",
      defer_loading: true,
      parameters: {
        type: "object",
        properties: { customer_id: { type: "string" } },
        required: ["customer_id"],
        additionalProperties: false,
      },
    },
  ],
};

const response = await openai.responses.create({
  model: "gpt-5.5",
  input: "List open orders for customer CUST-12345.",
  tools: [crmNamespace, { type: "tool_search" }],
  parallel_tool_calls: false,
});

console.log(response.output);
```

---

参考： [响应 API 参考](/zh-cn/rest/api/microsoft-foundry/azureopenai/responses?view=rest-microsoft-foundry-v1-preview&amp;preserve-view=true)

### 使用 API 密钥进行身份验证

API 密钥示例使用相同的命名空间和提示：

# [Python](#tab/python)
```python
import os
from openai import OpenAI

endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/"
openai = OpenAI(
    base_url=endpoint,
    api_key=os.getenv("AZURE_OPENAI_API_KEY"),
)

crm_namespace = {
    "type": "namespace",
    "name": "crm",
    "description": "CRM tools for customer lookup and order management.",
    "tools": [
        {
            "type": "function",
            "name": "get_customer_profile",
            "description": "Fetch a customer profile by customer ID.",
            "parameters": {
                "type": "object",
                "properties": {"customer_id": {"type": "string"}},
                "required": ["customer_id"],
                "additionalProperties": False,
            },
        },
        {
            "type": "function",
            "name": "list_open_orders",
            "description": "List open orders for a customer ID.",
            "defer_loading": True,
            "parameters": {
                "type": "object",
                "properties": {"customer_id": {"type": "string"}},
                "required": ["customer_id"],
                "additionalProperties": False,
            },
        },
    ],
}

response = openai.responses.create(
    model="gpt-5.5",
    input="List open orders for customer CUST-12345.",
    tools=[crm_namespace, {"type": "tool_search"}],
    parallel_tool_calls=False,
)

print(response.output)
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";
const openai = new OpenAI({
  baseURL: endpoint,
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const crmNamespace = {
  type: "namespace",
  name: "crm",
  description: "CRM tools for customer lookup and order management.",
  tools: [
    {
      type: "function",
      name: "get_customer_profile",
      description: "Fetch a customer profile by customer ID.",
      parameters: {
        type: "object",
        properties: { customer_id: { type: "string" } },
        required: ["customer_id"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "list_open_orders",
      description: "List open orders for a customer ID.",
      defer_loading: true,
      parameters: {
        type: "object",
        properties: { customer_id: { type: "string" } },
        required: ["customer_id"],
        additionalProperties: false,
      },
    },
  ],
};

const response = await openai.responses.create({
  model: "gpt-5.5",
  input: "List open orders for customer CUST-12345.",
  tools: [crmNamespace, { type: "tool_search" }],
  parallel_tool_calls: false,
});

console.log(response.output);
```

---

参考： [响应 API 参考](/zh-cn/rest/api/microsoft-foundry/azureopenai/responses?view=rest-microsoft-foundry-v1-preview&amp;preserve-view=true)

当模型需要延迟工具时，响应在函数调用之前包括两个输出项：

- `tool_search_call` 记录托管搜索步骤。
- `tool_search_output` 包含可调用的已加载子集。

以下示例显示了托管工具搜索响应：

```json
[
  {
    "type": "tool_search_call",
    "execution": "server",
    "call_id": null,
    "status": "completed",
    "arguments": {
      "paths": ["crm"]
    }
  },
  {
    "type": "tool_search_output",
    "execution": "server",
    "call_id": null,
    "status": "completed",
    "tools": [
      {
        "type": "namespace",
        "name": "crm",
        "description": "CRM tools for customer lookup and order management.",
        "tools": [
          {
            "type": "function",
            "name": "list_open_orders",
            "description": "List open orders for a customer ID.",
            "defer_loading": true,
            "parameters": {
              "type": "object",
              "properties": {
                "customer_id": { "type": "string" }
              },
              "required": ["customer_id"],
              "additionalProperties": false
            }
          }
        ]
      }
    ]
  },
  {
    "type": "function_call",
    "name": "list_open_orders",
    "namespace": "crm",
    "call_id": "call_abc123",
    "arguments": "{\"customer_id\":\"CUST-12345\"}"
  }
]
```

在托管模式下， `execution` 是 `server` 且 `call_id` 是 `null`。

对于复杂的任务，模型可以在一个 `tool_search_call`中加载多个命名空间或 MCP 服务器。 例如，如果任务需要来自不同命名空间的函数，模型可以在创建函数调用之前一起搜索和加载这些图面。

## 使用由客户端执行的工具搜索

客户端执行的工具搜索使应用程序可以完全控制工具发现。 当可用工具依赖于不便在初始 `tools` 列表中声明的信息时，请使用它。

使用 `tool_search` 和用于定义应用程序所需搜索参数的模式来配置 `execution: "client"` 工具：

# [Python](#tab/python)
```python
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/"
token_provider = get_bearer_token_provider(
    DefaultAzureCredential(), "https://ai.azure.com/.default"
)
openai = OpenAI(base_url=endpoint, api_key=token_provider)

first_response = openai.responses.create(
    model="gpt-5.5",
    input="Find the shipping ETA tool first, then use it for order_42.",
    tools=[
        {
            "type": "tool_search",
            "execution": "client",
            "description": "Find project tools needed to continue the task.",
            "parameters": {
                "type": "object",
                "properties": {"goal": {"type": "string"}},
                "required": ["goal"],
                "additionalProperties": False,
            },
        }
    ],
    parallel_tool_calls=False,
)

search_call = next(
    item for item in first_response.output
    if item.type == "tool_search_call"
)

loaded_tools = [
    {
        "type": "function",
        "name": "get_shipping_eta",
        "description": "Look up shipping ETA details for an order.",
        "defer_loading": True,
        "parameters": {
            "type": "object",
            "properties": {"order_id": {"type": "string"}},
            "required": ["order_id"],
            "additionalProperties": False,
        },
    }
]

second_response = openai.responses.create(
    model="gpt-5.5",
    input=[
        *first_response.output,
        {
            "type": "tool_search_output",
            "execution": "client",
            "call_id": search_call.call_id,
            "status": "completed",
            "tools": loaded_tools,
        },
    ],
)

print(second_response.output)
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";
import {
  DefaultAzureCredential,
  getBearerTokenProvider,
} from "@azure/identity";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";
const tokenProvider = getBearerTokenProvider(
  new DefaultAzureCredential(),
  "https://ai.azure.com/.default"
);
const openai = new OpenAI({
  baseURL: endpoint,
  apiKey: await tokenProvider(),
});

const firstResponse = await openai.responses.create({
  model: "gpt-5.5",
  input: "Find the shipping ETA tool first, then use it for order_42.",
  tools: [
    {
      type: "tool_search",
      execution: "client",
      description: "Find project tools needed to continue the task.",
      parameters: {
        type: "object",
        properties: { goal: { type: "string" } },
        required: ["goal"],
        additionalProperties: false,
      },
    },
  ],
  parallel_tool_calls: false,
});

const searchCall = firstResponse.output.find(
  (item) => item.type === "tool_search_call"
);
if (!searchCall) {
  throw new Error("The response didn't include a tool_search_call item.");
}

const loadedTools = [
  {
    type: "function",
    name: "get_shipping_eta",
    description: "Look up shipping ETA details for an order.",
    defer_loading: true,
    parameters: {
      type: "object",
      properties: { order_id: { type: "string" } },
      required: ["order_id"],
      additionalProperties: false,
    },
  },
];

const secondResponse = await openai.responses.create({
  model: "gpt-5.5",
  input: [
    ...firstResponse.output,
    {
      type: "tool_search_output",
      execution: "client",
      call_id: searchCall.call_id,
      status: "completed",
      tools: loadedTools,
    },
  ],
});

console.log(secondResponse.output);
```

---

参考： [响应 API 参考](/zh-cn/rest/api/microsoft-foundry/azureopenai/responses?view=rest-microsoft-foundry-v1-preview&amp;preserve-view=true)

若要使用 API 密钥身份验证，请将Microsoft Entra凭据替换为 API 密钥，如托管工具搜索 API 密钥示例所示。

第一轮，模型发出 `tool_search_call` 并停止：

```json
[
  {
    "type": "tool_search_call",
    "execution": "client",
    "call_id": "call_abc123",
    "status": "completed",
    "arguments": {
      "goal": "Find the shipping ETA tool for order_42."
    }
  }
]
```

应用程序执行搜索操作，并返回一个包含要加载工具的 `tool_search_output`：

```json
[
  {
    "type": "tool_search_output",
    "execution": "client",
    "call_id": "call_abc123",
    "status": "completed",
    "tools": [
      {
        "type": "function",
        "name": "get_shipping_eta",
        "description": "Look up shipping ETA details for an order.",
        "defer_loading": true,
        "parameters": {
          "type": "object",
          "properties": {
            "order_id": { "type": "string" }
          },
          "required": ["order_id"],
          "additionalProperties": false
        }
      }
    ]
  }
]
```

在下一轮，加载的工具可像正常函数一样调用：

```json
[
  {
    "type": "function_call",
    "name": "get_shipping_eta",
    "namespace": "get_shipping_eta",
    "call_id": "call_xyz456",
    "arguments": "{\"order_id\":\"order_42\"}"
  }
]
```

在客户端模式下， `execution` 是 `client` 并 `call_id` 已定义。 在相应的`call_id`中回显相同的`tool_search_call``tool_search_output`值。

## 应用高级使用模式

使用以下模式改进发现质量和控制工具添加到上下文的方式。

### 保持命名空间说明清晰

编写描述用例的简洁命名空间说明。 模型使用此说明决定何时从命名空间加载函数。 将更丰富的详细信息放在延迟函数说明中，仅在需要时加载。

### 了解所加载的内容

该 `tool_search_output.tools` 数组包含模型动态加载的工具。 模型可以在以后轮次调用这些工具，因此客户端模式不需要跨轮次再次加载同一工具。 此数组中没有的工具对模型不可用。

若要禁用已加载的工具，请将其从 `tool_search_output` 定义已加载工具集的项中删除。 更改已加载的工具集会导致模型的缓存从该点开始失效。

### 使用高级注入模式

大多数集成会在请求的 `tools` 参数中声明工具。 客户端执行的工具搜索还支持高级模式，其中应用程序返回原始请求中不存在的工具。 仔细验证返回的架构，并仅公开受信任的工具定义。

### 保留缓存

托管式和客户端执行的工具搜索都会在模型上下文窗口的末尾加载工具。 此位置可保留模型在请求之间的缓存，这可以降低成本并提高速度。

### 在输入中的特定点添加工具

使用 `additional_tools` 输入项，使工具在对话中的特定节点可用。 当应用程序在正常工具搜索流之外加载工具或需要保留在上一响应期间添加的工具的顺序时，此模式非常有用。

将 `role` 设置为 `developer`，并将这些工具包含在该项的 `tools` 数组中：

```json
{
  "type": "additional_tools",
  "role": "developer",
  "tools": [
    {
      "type": "function",
      "name": "get_customer",
      "description": "Look up a customer by ID.",
      "parameters": {
        "type": "object",
        "properties": {
          "customer_id": { "type": "string" }
        },
        "required": ["customer_id"],
        "additionalProperties": false
      }
    }
  ]
}
```

项目中 `additional_tools` 的工具仅在该项出现在输入中之后才可用。 在以后的请求中手动发送会话项时，请保留项目的位置，以便模型在对话中的同一时间点看到相同的工具。

## 工具搜索疑难解答

| 症状 | 可能的原因 | 修复 |
| --- | --- | --- |
| 模型不会查找延后调用的工具。 | 命名空间、MCP 服务器或函数说明与任务不匹配。 | 重写说明以声明图面的用途及其支持的任务。 |
| 托管搜索不会加载预期函数。 | 函数未标记 `defer_loading: true`，或者其父图面不清楚。 | 添加 `defer_loading: true` 和改进命名空间或 MCP 服务器说明。 |
| 客户端执行的搜索无法继续。 | `tool_search_output.call_id` 与前一个 `tool_search_call.call_id` 不匹配。 | 在输出项中原样输出相同的 `call_id`。 |
| 动态加载的工具不可调用。 | 该工具未包含在 `tool_search_output.tools`. | 返回输出数组中完整的受信任工具定义。 |
| 工具更新时提示缓存停止。 | 已加载的工具集或上下文中的较早项已发生更改。 | 使以前的项保持稳定，并在上下文末尾追加新加载的工具。 |