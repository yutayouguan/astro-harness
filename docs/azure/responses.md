---
layout: Conceptual
title: 使用 Azure OpenAI 响应 API - Microsoft Foundry | Microsoft Learn
canonicalUrl: https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/responses
schema: Conceptual
ai-usage: ai-assisted
author: alvinashcraft
breadcrumb_path: ../../../breadcrumb/azure-ai/toc.json
depot_name: Learn.azure-ai
description: 了解如何使用 Azure OpenAI 响应 API 通过 Python 或 REST 来创建、检索和删除有状态响应，包括流式处理功能和工具。
feedback_help_link_type: get-help-at-qna
feedback_help_link_url: https://learn.microsoft.com/answers/tags/133/azure
feedback_product_url: https://feedback.azure.com/d365community/forum/79b1327d-d925-ec11-b6e6-000d3a4f06a4
feedback_system: Standard
git_commit_id: 50853cc49f4e8e7a42a484337fa2e0c41e972fd9
gitcommit: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/50853cc49f4e8e7a42a484337fa2e0c41e972fd9/articles/foundry/openai/how-to/responses.md
learn_banner_products:
- azure
locale: zh-cn
manager: mcleans
ms.author: aashcraft
ms.collection: ce-skilling-ai-copilot
ms.custom:
- classic-and-new
- references_regions
- build-2025
- doc-kit-assisted
ms.date: 2026-08-18T00:00:00.0000000Z
ms.service: microsoft-foundry
ms.subservice: foundry-openai
ms.suite: office
ms.topic: how-to
ms.update-cycle: 90-days
original_content_git_url: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/live/articles/foundry/openai/how-to/responses.md
permissioned-type: public
recommendation_types:
- Training
- Certification
recommendations: true
site_name: Docs
uhfHeaderId: azure-ai-foundry
updated_at: 2026-08-28T13:51:00.0000000Z
ms.translationtype: HT
ms.contentlocale: zh-cn
loc_version: 2026-08-18T22:38:41.1012048Z
loc_source_id: Github-845157915#live
loc_file_id: Github-845157915.live.Learn.azure-ai.articles/foundry/openai/how-to/responses.md
page_type: conceptual
toc_rel: ../../toc.json
word_count: 14135
asset_id: foundry/openai/how-to/responses
item_type: Content
cmProducts:
- https://authoring-docs-microsoft.poolparty.biz/devrel/2d774b87-7dcb-40bf-a0b9-5a7a9efff0d1
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/8a6e4dad-7050-4ce7-83f9-eb4123577a54
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/1433a524-c01f-4b87-beab-670c040dea4f
spProducts:
- https://authoring-docs-microsoft.poolparty.biz/devrel/89dc5f37-0e4e-4b05-ad87-5fcd2b941a8a
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/0a5fc323-00ce-4c20-9095-41948f54c83f
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/312f1f05-a431-4193-8a4d-e6245d5966de
platformId: e1d58f77-18fa-d12d-45fa-d1696b52ee98
---

# 使用 Azure OpenAI 响应 API - Microsoft Foundry | Microsoft Learn

使用 Azure OpenAI 响应 API 生成有状态的多轮次响应。 它将聊天完成和助手 API 中的功能汇集在一个统一体验中。 使用 [多代理业务流程](responses-multi-agent) 将独立工作委托给并行子代理，或使用 [工具搜索](tool-search) 仅在模型需要它们时才加载工具定义。 响应 API 还支持 `computer-use-preview` 模型，该模型支持[计算机使用](../../../foundry-classic/openai/how-to/computer-use)。

有关完整的请求和响应参数说明，请参阅 [响应 API 参数参考](/zh-cn/rest/api/microsoft-foundry/azureopenai/responses?view=rest-microsoft-foundry-v1-preview&amp;preserve-view=true)。

## 先决条件

- 已部署Azure OpenAI 模型。
- 身份验证方法：
    - API 密钥（例如 `AZURE_OPENAI_API_KEY`），或
    - Microsoft Entra ID（建议）。
- 安装适用于语言的客户端库：
    - **Python**： `pip install openai azure-identity`
    - **.NET**： `dotnet add package OpenAI` 和 `dotnet add package Azure.Identity`
    - **JavaScript/TypeScript**： `npm install openai @azure/identity`
    - **Java**：向项目添加 `com.openai:openai-java` 和 `com.azure:azure-identity`。
- 对于 REST 示例，请设置 `AZURE_OPENAI_API_KEY` （API 密钥流）或 `AZURE_OPENAI_AUTH_TOKEN` （Microsoft Entra ID 流）。

## 支持的区域

在运行本文中的示例之前，请确认资源区域是否支持响应 API。 需要 v1 API 才能访问最新功能。 有关详细信息，请参阅 [API 版本生命周期](../api-version-lifecycle)。 有关 Foundry 代理服务区域支持，请参阅 [区域可用性表](../../agents/concepts/limits-quotas-regions#supported-regions)。 响应 API 目前在以下区域中可用：

- australiaeast
- brazilsouth
- canadacentral
- canadaeast
- centralus
- eastus
- eastus2
- francecentral
- 德国中西部
- 意大利北部
- japaneast
- japanwest
- koreacentral
- northcentralus
- 挪威东部
- polandcentral
- southafricanorth
- southcentralus
- 东南亚
- 南印度
- 西班牙中部 (spaincentral)
- swedencentral
- switzerlandnorth
- switzerlandwest
- uaenorth
- uksouth
- ukwest
- westcentralus
- westeurope
- westus
- westus2
- westus3

## 支持的模型

响应 API 支持以下模型：

- `gpt-5.6-sol` （版本： `2026-07-09`）
- `gpt-5.6-terra` （版本： `2026-07-09`）
- `gpt-5.6-luna` （版本： `2026-07-09`）
- `gpt-chat-latest`（版本：`2026-08-06`、`2026-06-24`、`2026-05-28``2026-05-05`）
- `gpt-5.5` （版本： `2026-04-24`）
- `gpt-5.4-nano` （版本： `2026-03-17`）
- `gpt-5.4-mini` （版本： `2026-03-17`）
- `gpt-5.4-pro` （版本：`2026-03-05`）
- `gpt-5.4` （版本：`2026-03-05`）
- `gpt-5.3-chat` （版本： `2026-03-03`）
- `gpt-5.3-codex` （版本： `2026-02-24`）
- `gpt-5.2-codex` （版本： `2026-01-14`）
- `gpt-5.2` （版本： `2025-12-11`）
- `gpt-5.2-chat` （版本： `2025-12-11`）
- `gpt-5.2-chat` （版本： `2026-02-10`）
- `gpt-5.1-codex-max` （版本： `2025-12-04`）
- `gpt-5.1` （版本： `2025-11-13`）
- `gpt-5.1-chat` （版本： `2025-11-13`）
- `gpt-5.1-codex` （版本： `2025-11-13`）
- `gpt-5.1-codex-mini` （版本： `2025-11-13`）
- `gpt-5-pro` （版本： `2025-10-06`）
- `gpt-5-codex` （版本： `2025-09-11`）
- `gpt-5` （版本： `2025-08-07`）
- `gpt-5-mini` （版本： `2025-08-07`）
- `gpt-5-nano` （版本： `2025-08-07`）
- `gpt-5-chat` （版本： `2025-08-07`）
- `gpt-5-chat` （版本： `2025-10-03`）
- `gpt-5-codex` （版本： `2025-09-15`）
- `gpt-4o` （版本： `2024-11-20`， `2024-08-06`， `2024-05-13`）
- `gpt-4o-mini` （版本： `2024-07-18`）
- `computer-use-preview`
- `gpt-4.1` （版本： `2025-04-14`）
- `gpt-4.1-nano` （版本： `2025-04-14`）
- `gpt-4.1-mini` （版本： `2025-04-14`）
- `gpt-image-1` （版本： `2025-04-15`）
- `gpt-image-1-mini` （版本： `2025-10-06`）
- `gpt-image-1.5` （版本： `2025-12-16`）
- `o1` （版本： `2024-12-17`）
- `o3-mini` （版本： `2025-01-31`）
- `o3` （版本： `2025-04-16`）
- `o4-mini` （版本： `2025-04-16`）

并非每个模型都可用于每个受支持的区域。 请查看 [模型页面](../../foundry-models/concepts/models-sold-directly-by-azure) 以获取模型地区可用性信息。

注意

当前不支持：

- 通过多轮编辑和流式处理生成图像。

存在以下已知问题：

- 现在 支持将 PDF 作为输入文件，但目前不支持将文件上传目的设置为 `user_data` 。
- 后台模式与流式处理一起使用时的性能问题。 Microsoft正在努力解决此问题。

## 生成文本响应

使用响应 API 生成简单的文本响应。 将 `YOUR-RESOURCE-NAME` 和 `MODEL_NAME` 替换为您的部署值。

# [Python](#tab/python)
```python
import os
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

# API key authentication
client = OpenAI(
    api_key=os.getenv("AZURE_OPENAI_API_KEY"),
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
)
response = client.responses.create(
    model="MODEL_NAME",
    input="This is a test."
)
print(response.model_dump_json(indent=2))

# Microsoft Entra ID authentication (recommended)
token_provider = get_bearer_token_provider(
    DefaultAzureCredential(), "https://ai.azure.com/.default"
)
client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=token_provider(),
)
response = client.responses.create(
    model="MODEL_NAME",
    input="This is a test."
)
print(response.model_dump_json(indent=2))
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems = { ResponseItem.CreateUserMessageItem("This is a test.") }
};
ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import { OpenAI } from "openai";
import { DefaultAzureCredential, getBearerTokenProvider } from "@azure/identity";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";

// API key authentication
const openai = new OpenAI({
  baseURL: endpoint,
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});
const response = await openai.responses.create({
  model: "MODEL_NAME",
  input: "This is a test."
});
console.log(response.output_text);

// Microsoft Entra ID authentication (recommended)
const tokenProvider = getBearerTokenProvider(
  new DefaultAzureCredential(),
  "https://ai.azure.com/.default"
);
const openaiEntra = new OpenAI({
  baseURL: endpoint,
  apiKey: await tokenProvider(),
});
const responseEntra = await openaiEntra.responses.create({
  model: "MODEL_NAME",
  input: "This is a test."
});
console.log(responseEntra.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

// Microsoft Entra ID authentication (recommended)
OpenAIClient openAIClientEntra = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(BearerTokenCredential.create(
        AuthenticationUtil.getBearerTokenSupplier(
            new DefaultAzureCredentialBuilder().build(),
            "https://ai.azure.com/.default")))
    .build();

ResponseCreateParams params = ResponseCreateParams.builder()
    .model("MODEL_NAME")
    .input("This is a test.")
    .build();
Response response = openAIClient.responses().create(params);
System.out.println(response.outputText());
```

# [REST](#tab/rest)
### Microsoft Entra ID

```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AZURE_OPENAI_AUTH_TOKEN" \
  -d '{
     "model": "MODEL_NAME",
     "input": "This is a test."
    }'
```

### API 密钥

```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
     "model": "MODEL_NAME",
     "input": "This is a test."
    }'
```

---

### 示例响应

```json
{
  "id": "resp_67cb32528d6881909eb2859a55e18a85",
  "created_at": 1741369938.0,
  "output_text": "Great! How can I help you today?",
  ...
}
```

## 检索响应

从以前的响应 API 调用中按其 ID 检索响应。

# [Python](#tab/python)
```python
import os
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

# API key authentication
client = OpenAI(
    api_key=os.getenv("AZURE_OPENAI_API_KEY"),
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
)
response = client.responses.retrieve("<response_id>")
print(response.model_dump_json(indent=2))

# Microsoft Entra ID authentication
token_provider = get_bearer_token_provider(
    DefaultAzureCredential(), "https://ai.azure.com/.default"
)
client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=token_provider,
)
response = client.responses.retrieve("<response_id>")
print(response.model_dump_json(indent=2))
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

string responseId = "<response_id>";
ResponseResult response = await openAIClient.GetResponseAsync(responseId);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import { OpenAI } from "openai";
import { DefaultAzureCredential, getBearerTokenProvider } from "@azure/identity";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";

// API key authentication
const openai = new OpenAI({
  baseURL: endpoint,
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});
const response = await openai.responses.retrieve("<response_id>");
console.log(response.output_text);

// Microsoft Entra ID authentication
const tokenProvider = getBearerTokenProvider(
  new DefaultAzureCredential(),
  "https://ai.azure.com/.default"
);
const openaiEntra = new OpenAI({
  baseURL: endpoint,
  apiKey: await tokenProvider(),
});
const responseEntra = await openaiEntra.responses.retrieve("<response_id>");
console.log(responseEntra.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

// Microsoft Entra ID authentication
OpenAIClient openAIClientEntra = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(BearerTokenCredential.create(
        AuthenticationUtil.getBearerTokenSupplier(
            new DefaultAzureCredentialBuilder().build(),
            "https://ai.azure.com/.default")))
    .build();

Response response = openAIClient.responses().retrieve("<response_id>");
System.out.println(response.outputText());
```

# [REST](#tab/rest)
### Microsoft Entra ID

```bash
curl -X GET https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses/<response_id> \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AZURE_OPENAI_AUTH_TOKEN"
```

### API 密钥

```bash
curl -X GET https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses/<response_id> \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY"
```

---

### 示例响应

```json
{
  "id": "resp_67cb61fa3a448190bcf2c42d96f0d1a8",
  "output_text": "Hello! How can I assist you today?",
  ...
}
```

## 删除响应

默认情况下，响应数据将保留 30 天。 按 ID 删除存储的响应。

# [Python](#tab/python)
```python
import os
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

# API key authentication
client = OpenAI(
    api_key=os.getenv("AZURE_OPENAI_API_KEY"),
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
)
response = client.responses.delete("<response_id>")
print(response)

# Microsoft Entra ID authentication
token_provider = get_bearer_token_provider(
    DefaultAzureCredential(), "https://ai.azure.com/.default"
)
client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=token_provider,
)
response = client.responses.delete("<response_id>")
print(response)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

string responseId = "<response_id>";
var result = await openAIClient.DeleteResponseAsync(responseId);
Console.WriteLine(result); // result.Deleted == true if successful
```

# [JavaScript](#tab/javascript)
```javascript
import { OpenAI } from "openai";
import { DefaultAzureCredential, getBearerTokenProvider } from "@azure/identity";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";

// API key authentication
const openai = new OpenAI({
  baseURL: endpoint,
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});
const result = await openai.responses.delete("<response_id>");
console.log(result);

// Microsoft Entra ID authentication
const tokenProvider = getBearerTokenProvider(
  new DefaultAzureCredential(),
  "https://ai.azure.com/.default"
);
const openaiEntra = new OpenAI({
  baseURL: endpoint,
  apiKey: await tokenProvider(),
});
const resultEntra = await openaiEntra.responses.delete("<response_id>");
console.log(resultEntra);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

// Microsoft Entra ID authentication
OpenAIClient openAIClientEntra = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(BearerTokenCredential.create(
        AuthenticationUtil.getBearerTokenSupplier(
            new DefaultAzureCredentialBuilder().build(),
            "https://ai.azure.com/.default")))
    .build();

Response result = openAIClient.responses().delete("<response_id>");
System.out.println(result);
```

# [REST](#tab/rest)
### Microsoft Entra ID

```bash
curl -X DELETE https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses/<response_id> \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AZURE_OPENAI_AUTH_TOKEN"
```

### API 密钥

```bash
curl -X DELETE https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses/<response_id> \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY"
```

---

## 将响应链接在一起

通过将上一次响应 ID 传递给 `previous_response_id` 来串联处理步骤。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

first_response = client.responses.create(
    model="MODEL_NAME",
    input="Define catastrophic forgetting."
)

second_response = client.responses.create(
    model="MODEL_NAME",
    previous_response_id=first_response.id,
    input="Explain it for a college freshman."
)

print(second_response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions firstOptions = new()
{
    Model = "MODEL_NAME",
    InputItems = { ResponseItem.CreateUserMessageItem("Define and explain the concept of catastrophic forgetting?") }
};
ResponseResult firstResponse = await openAIClient.CreateResponseAsync(firstOptions);
Console.WriteLine(firstResponse.GetOutputText());

CreateResponseOptions secondOptions = new()
{
    Model = "MODEL_NAME",
    PreviousResponseId = firstResponse.Id,
    InputItems = { ResponseItem.CreateUserMessageItem("Explain this at a level that could be understood by a college freshman") }
};
ResponseResult secondResponse = await openAIClient.CreateResponseAsync(secondOptions);
Console.WriteLine(secondResponse.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import { OpenAI } from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const firstResponse = await client.responses.create({
  model: "MODEL_NAME",
  input: "Define catastrophic forgetting."
});

const secondResponse = await client.responses.create({
  model: "MODEL_NAME",
  previous_response_id: firstResponse.id,
  input: "Explain it for a college freshman."
});

console.log(secondResponse.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Response first = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .input("Define and explain the concept of catastrophic forgetting?")
        .build());

Response second = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .previousResponseId(first.id())
        .input("Explain this at a level that could be understood by a college freshman.")
        .build());

second.output().stream()
    .flatMap(item -> item.message().stream())
    .flatMap(m -> m.content().stream())
    .flatMap(c -> c.outputText().stream())
    .forEach(t -> System.out.println(t.text()));
```

# [REST](#tab/rest)
```bash
# First turn
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": "Define catastrophic forgetting."
  }'

# Follow-up turn using previous_response_id from the first call
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "previous_response_id": "<response_id>",
    "input": "Explain it for a college freshman."
  }'
```

---

### 手动串联响应

或者，可以在下一个请求中手动转发输出项。

```python
import os
from openai import OpenAI

client = OpenAI(
  base_url = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

inputs = [{"type": "message", "role": "user", "content": "Define and explain the concept of catastrophic forgetting?"}]

response = client.responses.create(
    model="gpt-4o",  # replace with your model deployment name
    input=inputs
)

inputs += response.output

inputs.append({"role": "user", "type": "message", "content": "Explain this at a level that could be understood by a college freshman"})

second_response = client.responses.create(
  model="MODEL_NAME",
    input=inputs
)

print(second_response.model_dump_json(indent=2))
```

## 压缩响应

压缩可减少输入上下文，同时保留后续轮次的基本状态。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
  base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

compacted = client.responses.compact(
  model="MODEL_NAME",
  input=[
    {"role": "user", "content": "Create a simple landing page for a dog cafe."},
    {
      "id": "msg_001",
      "type": "message",
      "status": "completed",
      "role": "assistant",
      "content": [{"type": "output_text", "text": "..."}],
    },
  ]
)

follow_up = client.responses.create(
  model="MODEL_NAME",
  input=[*compacted.output, {"role": "user", "content": "Add a booking form."}]
)
print(follow_up.output_text)
```

# [C#](#tab/csharp)
注意

.NET SDK 目前尚未为响应压缩提供强类型接口。 请参阅 **REST** 选项卡以了解调用格式，或使用 `BinaryContent` JSON 直接调用该协议方法。

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const compacted = await client.responses.compact({
  model: "MODEL_NAME",
  input: [
    { role: "user", content: "Create a simple landing page for a dog cafe." },
    {
      id: "msg_001",
      type: "message",
      status: "completed",
      role: "assistant",
      content: [{ type: "output_text", text: "..." }],
    },
  ],
});

const followUp = await client.responses.create({
  model: "MODEL_NAME",
  input: [...compacted.output, { role: "user", content: "Add a booking form." }],
});
console.log(followUp.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.CompactedResponse;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCompactParams;
import com.openai.models.responses.ResponseCreateParams;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Response initial = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .input("Create a simple landing page for a dog cafe.")
        .build());

CompactedResponse compacted = openAIClient.responses().compact(
    ResponseCompactParams.builder()
        .model("MODEL_NAME")
        .previousResponseId(initial.id())
        .build());

Response followUp = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .previousResponseId(compacted.id())
        .input("Add a booking form.")
        .build());

System.out.println(followUp.outputText());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses/compact \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": [
      {"role": "user", "content": "Create a simple landing page for a dog cafe."},
      {
      "id": "msg_001",
      "type": "message",
      "status": "completed",
      "role": "assistant",
      "content": [{"type": "output_text", "text": "..."}]
      }
    ]
    }'
```

---

### 使用返回的项进行压缩

可以压缩先前请求返回的所有项，例如推理、消息、函数调用等。

```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses/compact \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AZURE_OPENAI_AUTH_TOKEN" \
  -d '{
        "model": "MODEL_NAME",
        "input": [
          {
            "role"   : "user",
            "content": "Create a simple landing page for a dog petting café."
          },
          {
            "id": "msg_001",
            "type": "message",
            "status": "completed",
            "content": [
              {
                "type": "output_text",
                "annotations": [],
                "logprobs": [],
                "text": "Below is a single file, ready-to-use landing page for a dog petting café:..."
              }
            ],
            "role": "assistant"
          }
        ]
    }'
```

```python
# Use the compacted output as input for the next turn.
next_response = client.responses.create(
  model="MODEL_NAME",
  input=[*compacted.output, {"role": "user", "content": "Add opening hours."}],
)
print(next_response.output_text)
```

### 使用以前的响应 ID 压缩

还可以使用以前的响应 ID 压缩。

```python
initial_response = client.responses.create(
  model="MODEL_NAME",
  input="What is the size of France?"
)

compacted_response = client.responses.compact(
  model="MODEL_NAME",
  previous_response_id=initial_response.id
)

follow_up_response = client.responses.create(
  model="MODEL_NAME",
  input=[
    *compacted_response.output,
    {"role": "user", "content": "What is the capital?"}
  ]
)
print(follow_up_response.output_text)
```

### 服务器端压缩

你还可以通过设置`POST /responses` 和`client.responses.create`，直接在响应中使用服务器端压缩（`context_management` 或`compact_threshold`）。

- 当输出令牌计数超过配置的阈值时，响应 API 会自动运行压缩。
- 在此模式下，无需单独调用 `/responses/compact` 。
- 响应包括加密压缩项。
- 在响应创建请求上设置 store=false 时，服务器端压缩将起作用。

压缩项使用较少的令牌将基本的以前状态和推理推进到下一轮次。 这是不透明的，不应是人类可读的。

如果使用无状态输入数组链接过程，请像往常一样追加输出项。 如果使用 `previous_response_id`，则每次轮次仅传递新用户消息。 在这两种模式中，压缩项携带下一个窗口所需的上下文。

提示

将输出项追加到以前的输入项后，可以删除最近压缩项之前的项，以保持请求更小并减少长尾延迟。 最新的压缩项包含了继续对话所需的上下文。 如果使用 `previous_response_id` 串联，请不要手动裁剪。

#### Flow

1. 照常调用 `responses` 。 添加 `context_management` 并通过 `compact_threshold` 启用服务器端压缩。
2. 如果输出超过阈值，服务将触发压缩，在输出流中发出压缩项，并在继续推理之前修剪上下文。
3. 使用以下模式之一继续聊天：
    1. 无状态输入数组链式处理：将输出项（包括压缩项）添加到下一个输入数组中。
    2. `previous_response_id` 链接：在每个轮次仅传递新用户消息，并将最新的响应 ID 一并传递下去。

#### 例子

```python
conversation = [
  {
    "type": "message",
    "role": "user",
    "content": "Let's begin a long coding task.",
  }
]

while keep_going:
  response = client.responses.create(
    model="MODEL_NAME",
    input=conversation,
    store=False,
    context_management=[{"type": "compaction", "compact_threshold": 200000}],
  )

  conversation.append(
    {
      "type": "message",
       "role": "user",
      "content": get_next_user_input(),
    }
  )
```

## 流媒体

通过设置 `stream=true`来流式传输响应。 该服务会发布增量事件，您可以订阅这些事件以逐令牌渲染输出。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

stream = client.responses.create(
    model="MODEL_NAME",
    input="Summarize Azure OpenAI Responses API in one sentence.",
    stream=True,
)

for event in stream:
    if event.type == "response.output_text.delta":
        print(event.delta, end="")
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems = { ResponseItem.CreateUserMessageItem("Summarize Azure OpenAI Responses API in one sentence.") },
    StreamingEnabled = true
};

await foreach (StreamingResponseUpdate update in openAIClient.CreateResponseStreamingAsync(options))
{
    if (update is StreamingResponseOutputTextDeltaUpdate textDelta)
    {
        Console.Write(textDelta.Delta);
    }
    else if (update is StreamingResponseCompletedUpdate completed)
    {
        Console.WriteLine();
        Console.WriteLine($"[done] response id: {completed.Response.Id}");
    }
}
```

# [JavaScript](#tab/javascript)
```javascript
import { OpenAI } from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const stream = await client.responses.create({
  model: "MODEL_NAME",
  input: "Summarize Azure OpenAI Responses API in one sentence.",
  stream: true,
});

for await (const event of stream) {
  if (event.type === "response.output_text.delta") {
    process.stdout.write(event.delta);
  }
}
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.core.http.StreamResponse;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseStreamEvent;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

ResponseCreateParams params = ResponseCreateParams.builder()
    .model("MODEL_NAME")
    .input("This is a test")
    .build();

try (StreamResponse<ResponseStreamEvent> stream = openAIClient.responses().createStreaming(params)) {
    stream.stream()
        .flatMap(event -> event.outputTextDelta().stream())
        .forEach(delta -> System.out.print(delta.delta()));
}
```

# [REST](#tab/rest)
```bash
curl -N -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": "Summarize Azure OpenAI Responses API in one sentence.",
    "stream": true
  }'
```

---

在流式处理期间，如果错误发生在流中，系统会尝试内部重试以解决错误。 如果请求仍然失败，服务将遇到不可恢复的错误。 在这些情况下，HTTP 响应代码为 200（成功），但流中的错误事件包含有关中流错误的详细信息。

服务器返回以下错误类型：

| 错误类型 | 可以翻译为 |
| --- | --- |
| `server_error` | 返回代码 `500` |
| `too_many_requests` | 返回代码 `429` |
| `forbidden` | 返回代码 `403` |
| `user_error` | 返回代码 `400` |

示例错误事件：

```json
{
  "type": "error",
  "error": {
    "type": "too_many_requests",
    "code": "no_capacity",
    "headers": {
      "skip-error-remapping": "true"
    },
    "message": "The system is currently experiencing high demand and cannot process your request. Your request exceeds the maximum usage size allowed during peak load. For improved capacity reliability, consider switching to Provisioned Throughput.",
    "param": null
  }
}
```

应用程序应检测这些错误，并优雅地停止或重新启动流传输。 对于流媒体响应失败期间生成的令牌，将不收取费用。

## 结构化输出

使用结构化输出根据 JSON 架构发出响应。 在 Responses API 请求中，在 `text.format` 中定义架构。 聊天补全功能改用 `response_format` 代替。 有关 SDK 和 REST 示例、支持的架构约束和限制，请参阅 [结构化输出](structured-outputs)。

## 函数调用

响应 API 支持函数调用。

# [Python](#tab/python)
```python
import os
import json
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

response = client.responses.create(
    model="MODEL_NAME",
    tools=[
        {
            "type": "function",
            "name": "get_weather",
            "description": "Get weather for a location",
            "parameters": {
                "type": "object",
                "properties": {"location": {"type": "string"}},
                "required": ["location"],
            },
        }
    ],
    input="What is the weather in San Francisco?",
)

tool_outputs = []
for item in response.output:
    if item.type == "function_call" and item.name == "get_weather":
        args = json.loads(item.arguments)
        weather = {"location": args["location"], "temperature": "70 F"}
        tool_outputs.append(
            {
                "type": "function_call_output",
                "call_id": item.call_id,
                "output": json.dumps(weather),
            }
        )

final_response = client.responses.create(
    model="MODEL_NAME",
    previous_response_id=response.id,
    input=tool_outputs,
)

print(final_response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using System.Text.Json;
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

FunctionTool getWeatherTool = ResponseTool.CreateFunctionTool(
    functionName: "get_weather",
    functionParameters: BinaryData.FromBytes("""
        {
          "type": "object",
          "properties": {
            "location": { "type": "string", "description": "The city, e.g. Boston, MA" }
          },
          "required": ["location"]
        }
        """u8.ToArray()),
    strictModeEnabled: false,
    functionDescription: "Get the current weather for a location.");

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems = { ResponseItem.CreateUserMessageItem("What is the weather in San Francisco?") },
    Tools = { getWeatherTool }
};

ResponseResult response = await openAIClient.CreateResponseAsync(options);

foreach (ResponseItem item in response.OutputItems)
{
    if (item is FunctionCallResponseItem call && call.FunctionName == "get_weather")
    {
        using JsonDocument args = JsonDocument.Parse(call.FunctionArguments);
        string location = args.RootElement.GetProperty("location").GetString();
        string toolOutput = $"{{ \"location\": \"{location}\", \"temperature\": \"70 F\" }}";

        CreateResponseOptions followUp = new()
        {
            Model = "MODEL_NAME",
            PreviousResponseId = response.Id,
            InputItems = { ResponseItem.CreateFunctionCallOutputItem(call.CallId, toolOutput) },
            Tools = { getWeatherTool }
        };

        ResponseResult finalResponse = await openAIClient.CreateResponseAsync(followUp);
        Console.WriteLine(finalResponse.GetOutputText());
    }
}
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  tools: [
    {
      type: "function",
      name: "get_weather",
      description: "Get weather for a location",
      parameters: {
        type: "object",
        properties: { location: { type: "string" } },
        required: ["location"],
      },
    },
  ],
  input: "What is the weather in San Francisco?",
});

const toolOutputs = [];
for (const item of response.output ?? []) {
  if (item.type === "function_call" && item.name === "get_weather") {
    const args = JSON.parse(item.arguments);
    toolOutputs.push({
      type: "function_call_output",
      call_id: item.call_id,
      output: JSON.stringify({ location: args.location, temperature: "70 F" }),
    });
  }
}

const finalResponse = await client.responses.create({
  model: "MODEL_NAME",
  previous_response_id: response.id,
  input: toolOutputs,
});

console.log(finalResponse.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.fasterxml.jackson.annotation.JsonPropertyDescription;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseFunctionToolCall;
import com.openai.models.responses.ResponseInputItem;
import java.util.ArrayList;
import java.util.List;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

// Strongly-typed function parameter class.
class GetWeather {
    @JsonPropertyDescription("City and country, for example, Paris, France")
    public String location;
}

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .input("What is the weather like in Paris today?")
        .addTool(GetWeather.class)
        .build());

List<ResponseInputItem> followUp = new ArrayList<>();
response.output().forEach(item -> {
    if (item.isFunctionCall()) {
        ResponseFunctionToolCall call = item.asFunctionCall();
        // Execute the tool with call.arguments() and capture the result.
        String result = "{\"temperature\":\"22 C\",\"conditions\":\"Sunny\"}";
        followUp.add(ResponseInputItem.ofFunctionCallOutput(
            ResponseInputItem.FunctionCallOutput.builder()
                .callId(call.callId())
                .output(result)
                .build()));
    }
});

Response finalResponse = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .previousResponseId(response.id())
        .inputOfResponse(followUp)
        .addTool(GetWeather.class)
        .build());

System.out.println(finalResponse.outputText());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "tools": [
      {
        "type": "function",
        "name": "get_weather",
        "description": "Get weather for a location",
        "parameters": {
          "type": "object",
          "properties": {"location": {"type": "string"}},
          "required": ["location"]
        }
      }
    ],
    "input": "What is the weather in San Francisco?"
  }'
```

---

## 处理防护栏和内容筛选

防护栏（内容筛选器）在部署级别应用，并在每次响应 API 调用上自动运行，因此它们保护你发送的输入和模型生成的输出。 您可以分别配置护栏。 有关详细信息，请参阅 [配置护栏和控制](../../guardrails/how-to-create-guardrails)。 本部分介绍如何在调用响应 API 时检测和处理防护措施结果。

Responses API 呈现护栏结果的方式与聊天补全不同。 聊天补全返回的 `prompt_filter_results` 和 `content_filter_results` 字段不再使用，响应对象现包含顶层的 `content_filters` 数组。 每个条目描述一个筛选器结果。

| 领域 | Description |
| --- | --- |
| `blocked` | 内容是否被阻止。 |
| `source_type` | 该结果适用于`prompt`（输入）还是`completion`（输出）。 |
| `content_filter_results` | 类别结果，例如带有严重程度级别的`hate`、`sexual`、`violence`和`self_harm`，以及可选类别，如`jailbreak`、`indirect_attack`、`protected_material_text`和`protected_material_code`。 |
| `content_filter_offsets` | 结果适用的字符偏移量。 |

注意

该`content_filters`数组是一个Microsoft Foundry 扩展，该扩展不属于基本 OpenAI 响应架构，因此 SDK 不会为其公开类型化属性。 将其作为原始字段或附加字段读取，如以下示例所示。

### 检测输入阻塞

当护栏阻止输入时，API 将返回 HTTP 400 错误并显示代码 `content_filter`。 捕获此错误，以优雅处理被阻断的提示。

# [Python](#tab/python)
```python
import os
from openai import OpenAI, BadRequestError

client = OpenAI(
    api_key=os.getenv("AZURE_OPENAI_API_KEY"),
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
)

# A blocked prompt raises BadRequestError with the code "content_filter"
try:
    response = client.responses.create(
        model="MODEL_NAME",
        input="This is a test."
    )
    print(response.output_text)
except BadRequestError as error:
    if error.code == "content_filter":
        print("The prompt was blocked by a guardrail.")
    else:
        raise
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using OpenAI.Responses;
using System.ClientModel;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems = { ResponseItem.CreateUserMessageItem("This is a test.") }
};

// A blocked prompt throws ClientResultException with HTTP 400
try
{
    ResponseResult response = await openAIClient.CreateResponseAsync(options);
    Console.WriteLine(response.GetOutputText());
}
catch (ClientResultException error) when (error.Status == 400)
{
    Console.WriteLine("The prompt was blocked by a guardrail.");
}
```

# [JavaScript](#tab/javascript)
```javascript
import { OpenAI, APIError } from "openai";

const openai = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

// A blocked prompt throws APIError with the code "content_filter"
try {
  const response = await openai.responses.create({
    model: "MODEL_NAME",
    input: "This is a test."
  });
  console.log(response.output_text);
} catch (error) {
  if (error instanceof APIError && error.code === "content_filter") {
    console.log("The prompt was blocked by a guardrail.");
  } else {
    throw error;
  }
}
```

# [Java](#tab/java)
```java
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.AzureApiKeyCredential;
import com.openai.errors.BadRequestException;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl("https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1")
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

ResponseCreateParams params = ResponseCreateParams.builder()
    .model("MODEL_NAME")
    .input("This is a test.")
    .build();

// A blocked prompt throws BadRequestException (HTTP 400)
try {
    Response response = openAIClient.responses().create(params);
    System.out.println(response.outputText());
} catch (BadRequestException error) {
    System.out.println("The prompt was blocked by a guardrail.");
}
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": "This is a test."
  }'
```

当护栏阻止输入时，API 会返回 HTTP 400，其中包含以下代码 `content_filter`：

```json
{
  "error": {
    "code": "content_filter",
    "message": "The response was filtered due to the prompt triggering content management policy."
  }
}
```

---

### 读取护栏批注

请求成功后，从响应中读取 `content_filters` 数组，以检查输入和输出的防护措施结果。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    api_key=os.getenv("AZURE_OPENAI_API_KEY"),
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
)
response = client.responses.create(
    model="MODEL_NAME",
    input="This is a test."
)

# content_filters is an Azure extension, so read it from model_extra
content_filters = response.model_extra.get("content_filters", [])
for result in content_filters:
    print(f"Source: {result['source_type']}, Blocked: {result['blocked']}")
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using OpenAI.Responses;
using System.ClientModel;
using System.ClientModel.Primitives;
using System.Text.Json;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems = { ResponseItem.CreateUserMessageItem("This is a test.") }
};

// content_filters has no typed property, so parse it from the raw response
ClientResult<ResponseResult> result = await openAIClient.CreateResponseAsync(options);
using JsonDocument doc = JsonDocument.Parse(result.GetRawResponse().Content);
if (doc.RootElement.TryGetProperty("content_filters", out JsonElement filters))
{
    foreach (JsonElement filter in filters.EnumerateArray())
    {
        string source = filter.GetProperty("source_type").GetString()!;
        bool blocked = filter.GetProperty("blocked").GetBoolean();
        Console.WriteLine($"Source: {source}, Blocked: {blocked}");
    }
}
```

# [JavaScript](#tab/javascript)
```javascript
import { OpenAI } from "openai";

const openai = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});
const response = await openai.responses.create({
  model: "MODEL_NAME",
  input: "This is a test."
});

// content_filters is an Azure extension not in the typed response
const contentFilters = response.content_filters ?? [];
for (const result of contentFilters) {
  console.log(`Source: ${result.source_type}, Blocked: ${result.blocked}`);
}
```

# [Java](#tab/java)
```java
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.AzureApiKeyCredential;
import com.openai.core.JsonValue;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl("https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1")
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

ResponseCreateParams params = ResponseCreateParams.builder()
    .model("MODEL_NAME")
    .input("This is a test.")
    .build();
Response response = openAIClient.responses().create(params);

// content_filters has no typed accessor, so read it from additional properties
JsonValue contentFilters = response._additionalProperties().get("content_filters");
System.out.println(contentFilters);
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": "This is a test."
  }'
```

数组 `content_filters` 显示在响应对象上：

```json
{
  "id": "resp_<id>",
  "content_filters": [
    {
      "source_type": "prompt",
      "blocked": false,
      "content_filter_results": {
        "hate": { "filtered": false, "severity": "safe" },
        "self_harm": { "filtered": false, "severity": "safe" },
        "sexual": { "filtered": false, "severity": "safe" },
        "violence": { "filtered": false, "severity": "safe" }
      }
    }
  ]
}
```

---

若要了解有关护栏类别和严重性级别的详细信息，请参阅 [防护栏概述](../../guardrails/guardrails-overview) 和 [处理批注](../../guardrails/how-to-create-guardrails#work-with-annotations)。

## 代码解释器

代码解释器工具使模型能够在安全的沙盒环境中编写和执行Python代码。 它支持一系列高级任务，包括：

- 处理具有不同数据格式和结构的文件
- 生成包含数据和可视化效果的文件（例如图形）
- 以迭代方式编写和运行代码来解决问题 — 模型可以调试和重试代码，直到成功
- 通过启用图像转换（例如裁剪、缩放和旋转）增强受支持模型中的视觉推理（例如 o3、o4-mini）
- 此工具特别适用于涉及数据分析、数学计算和代码生成的方案。

```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
        "model": "MODEL_NAME",
        "tools": [
            { "type": "code_interpreter", "container": {"type": "auto"} }
        ],
        "instructions": "You are a personal math tutor. When asked a math question, write and run code using the python tool to answer the question.",
        "input": "I need to solve the equation 3x + 11 = 14. Can you help me?"
    }'
```

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

response = client.responses.create(
    model="MODEL_NAME",
    tools=[{"type": "code_interpreter", "container": {"type": "auto"}}],
    instructions="You are a math tutor. Write and run Python code to solve math problems.",
    input="Solve 3x + 11 = 14."
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Containers;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CodeInterpreterToolContainer container = new(
    CodeInterpreterToolContainerConfiguration.CreateAutomaticContainerConfiguration());
CodeInterpreterTool codeInterpreterTool = new(container);

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems =
    {
        ResponseItem.CreateUserMessageItem("Solve 3x + 11 = 14.")
    },
    Tools = { codeInterpreterTool }
};

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  tools: [{ type: "code_interpreter", container: { type: "auto" } }],
  instructions: "You are a math tutor. Write and run Python code to solve math problems.",
  input: "Solve 3x + 11 = 14.",
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.Tool;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Tool codeInterpreter = Tool.ofCodeInterpreter(
    Tool.CodeInterpreter.builder()
        .container(Tool.CodeInterpreter.Container.ofCodeInterpreterToolAuto(
            Tool.CodeInterpreter.Container.CodeInterpreterToolAuto.builder().build()))
        .build());

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .input("Solve 3x + 11 = 14.")
        .addTool(codeInterpreter)
        .build());

System.out.println(response.outputText());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "tools": [{"type": "code_interpreter", "container": {"type": "auto"}}],
    "instructions": "You are a math tutor. Write and run Python code to solve math problems.",
    "input": "Solve 3x + 11 = 14."
  }'
```

---

### 容器

重要

除了与使用 Azure OpenAI 相关的基于令牌的费用之外，代码解释器还有[额外的费用](https://azure.microsoft.com/pricing/details/cognitive-services/openai-service/)。 如果响应 API 在两个不同的线程中同时调用代码解释器，则会创建两个代码解释器会话。 每个会话会产生每分钟的费用，最低费用为 5 分钟。 只要在 20 分钟的空闲超时内访问其容器，会话就保持活动状态。

代码解释器工具需要一个容器，即一个完全沙盒的虚拟机，模型可以在其中执行Python代码。 容器可以包括上传的文件或执行期间生成的文件。

若要创建容器，请在创建新的 Response 对象时在工具配置中指定 `"container": { "type": "auto", "file_ids": ["file-1", "file-2"] }` 。 这会自动创建新容器或重复使用模型上下文中先前code\_interpreter\_call的活动容器。 API 输出中的 `code_interpreter_call` 将包含生成的 `container_id`。 如果未使用 20 分钟，此容器将过期。

以下文件限制适用：

- 请求最多可以包含 50 个文件 ID。
- 容器总共可以容纳多达 1,000 个文件，包括代码解释器生成的输入文件和文件。

### 文件输入和输出

运行代码解释器时，模型可以创建自己的文件。 例如，如果要求它构造绘图或创建 CSV，它会直接在容器上创建这些映像。 它将在其下一条消息的批注中引用这些文件。

模型输入中的任何文件都会自动上传到容器。 无需明确地将其上传到容器。

### 支持的文件

| 文件格式 | MIME 类型 |
| --- | --- |
| `.c` | text/x-c |
| `.cs` | text/x-csharp |
| `.cpp` | text/x-c++ |
| `.csv` | text/csv |
| `.doc` | application/msword |
| `.docx` | application/vnd.openxmlformats-officedocument.wordprocessingml.document |
| `.html` | text/html |
| `.java` | text/x-java |
| `.json` | application/json |
| `.md` | text/markdown |
| `.pdf` | application/pdf |
| `.php` | text/x-php |
| `.pptx` | application/vnd.openxmlformats-officedocument.presentationml.presentation |
| `.py` | text/x-python |
| `.py` | text/x-script.python |
| `.rb` | text/x-ruby |
| `.tex` | text/x-tex |
| `.txt` | text/plain |
| `.css` | text/css |
| `.js` | text/JavaScript |
| `.sh` | application/x-sh |
| `.ts` | application/TypeScript |
| `.csv` | application/csv |
| `.jpeg` | image/jpeg |
| `.jpg` | image/jpeg |
| `.gif` | image/gif |
| `.pkl` | application/octet-stream |
| `.png` | image/png |
| `.tar` | application/x-tar |
| `.xlsx` | application/vnd.openxmlformats-officedocument.spreadsheetml.sheet |
| `.xml` | application/xml 或“text/xml” |
| `.zip` | application/zip |

## 列出输入项

获取随响应一起发送的输入项。 这可用于检查完整对话上下文，包括模型添加的任何项（例如函数调用或压缩项）。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

items = client.responses.input_items.list("<response_id>")
print(items.model_dump_json(indent=2))
```

# [C#](#tab/csharp)
注意

.NET SDK 仅将此终结点公开为协议方法。 请参阅调用形状的 **REST** 选项卡，或直接调用协议方法。

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const items = await client.responses.inputItems.list("<response_id>");
console.log(JSON.stringify(items, null, 2));
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.inputitems.ResponseInputItemListPage;
import com.openai.models.responses.inputitems.ResponseInputItemListParams;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

ResponseInputItemListPage page = openAIClient.responses().inputItems().list(
    ResponseInputItemListParams.builder()
        .responseId("<response_id>")
        .build());

page.autoPager().stream().forEach(item -> System.out.println(item));
```

# [REST](#tab/rest)
```bash
curl -X GET https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses/<response_id>/input_items \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY"
```

---

### 示例响应

```json
{
  "object": "list",
  "data": [
    {
      "id": "msg_...",
      "type": "message",
      "role": "user",
      "content": [{"type": "input_text", "text": "This is a test."}]
    }
  ]
}
```

## 图像输入

启用视觉的模型可以解释图像以及文本。 它们可以识别对象、形状、颜色和纹理，并读取图像中包含的文本，但受本文后面列出的限制的约束。

可以通过以下任一方式为请求提供图像作为输入：

- 图像文件的完全限定 URL
- 经过 Base64 编码的数据 URI
- 使用[文件 API](/zh-cn/rest/api/microsoft-foundry/azureopenai/files?view=rest-microsoft-foundry-v1-preview&amp;preserve-view=true) 创建的文件 ID

### 图像网址

引用一张托管在公开 URL 上的图像。 模型会获取该图像，并将其作为输入内容的一部分。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

response = client.responses.create(
    model="MODEL_NAME",
    input=[
        {
            "role": "user",
            "content": [
                {"type": "input_text", "text": "What is in this image?"},
                {"type": "input_image", "image_url": "<image_url>"}
            ]
        }
    ]
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems =
    {
        ResponseItem.CreateUserMessageItem(
        [
            ResponseContentPart.CreateInputTextPart("What is in this image?"),
            ResponseContentPart.CreateInputImagePart(new Uri("<image_url>"))
        ])
    }
};

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  input: [
    {
      role: "user",
      content: [
        { type: "input_text", text: "What is in this image?" },
        { type: "input_image", image_url: "<image_url>" }
      ],
    },
  ],
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseInputImage;
import com.openai.models.responses.ResponseInputItem;
import java.util.List;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

ResponseInputImage image = ResponseInputImage.builder()
    .detail(ResponseInputImage.Detail.AUTO)
    .imageUrl("<image_url>")
    .build();

ResponseInputItem userMsg = ResponseInputItem.ofMessage(
    ResponseInputItem.Message.builder()
        .role(ResponseInputItem.Message.Role.USER)
        .addInputTextContent("What is in this image?")
        .addContent(image)
        .build());

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .inputOfResponse(List.of(userMsg))
        .build());

System.out.println(response.outputText());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": [
      {
        "role": "user",
        "content": [
          {"type": "input_text", "text": "What is in this image?"},
          {"type": "input_image", "image_url": "<image_url>"}
        ]
      }
    ]
  }'
```

---

### Base64 编码的图像

通过将图像字节编码为 base64 数据 URI 来内联发送图像。 如果映像未托管在公共 URL 上，或者想要避免额外的网络提取，请使用此模式。

# [Python](#tab/python)
```python
import base64
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

with open("path_to_your_image.jpg", "rb") as image_file:
    base64_image = base64.b64encode(image_file.read()).decode("utf-8")

response = client.responses.create(
    model="MODEL_NAME",
    input=[
        {
            "role": "user",
            "content": [
                {"type": "input_text", "text": "What is in this image?"},
                {"type": "input_image", "image_url": f"data:image/jpeg;base64,{base64_image}"}
            ]
        }
    ]
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using System.IO;
using System.Threading.Tasks;
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

byte[] imageBytes = await File.ReadAllBytesAsync("path_to_your_image.jpg");
BinaryData imageData = BinaryData.FromBytes(imageBytes);

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems =
    {
        ResponseItem.CreateUserMessageItem(
        [
            ResponseContentPart.CreateInputTextPart("What is in this image?"),
            ResponseContentPart.CreateInputImagePart(imageData, "image/jpeg")
        ])
    }
};

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import { readFileSync } from "node:fs";
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const base64Image = readFileSync("path_to_your_image.jpg").toString("base64");

const response = await client.responses.create({
  model: "MODEL_NAME",
  input: [
    {
      role: "user",
      content: [
        { type: "input_text", text: "What is in this image?" },
        { type: "input_image", image_url: `data:image/jpeg;base64,${base64Image}` }
      ],
    },
  ],
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseInputImage;
import com.openai.models.responses.ResponseInputItem;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.Base64;
import java.util.List;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

byte[] bytes = Files.readAllBytes(Paths.get("cat.jpg"));
String dataUrl = "data:image/jpeg;base64," + Base64.getEncoder().encodeToString(bytes);

ResponseInputImage image = ResponseInputImage.builder()
    .detail(ResponseInputImage.Detail.AUTO)
    .imageUrl(dataUrl)
    .build();

ResponseInputItem userMsg = ResponseInputItem.ofMessage(
    ResponseInputItem.Message.builder()
        .role(ResponseInputItem.Message.Role.USER)
        .addInputTextContent("What is in this image?")
        .addContent(image)
        .build());

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .inputOfResponse(List.of(userMsg))
        .build());

System.out.println(response.outputText());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": [
      {
        "role": "user",
        "content": [
          {"type": "input_text", "text": "What is in this image?"},
          {"type": "input_image", "image_url": "data:image/jpeg;base64,<BASE64_IMAGE>"}
        ]
      }
    ]
  }'
```

---

### 文件标识

使用 `purpose="assistants"`文件 API 上传图像，然后在请求中引用返回的文件 ID。 如果要在多个请求之间重复使用同一映像，而无需重新发送其字节，此方法非常有用。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

def create_file(file_path):
    with open(file_path, "rb") as file_content:
        result = client.files.create(
            file=file_content,
            purpose="assistants",
        )
        return result.id

file_id = create_file("path_to_your_image.jpg")

response = client.responses.create(
    model="MODEL_NAME",
    input=[
        {
            "role": "user",
            "content": [
                {"type": "input_text", "text": "What is in this image?"},
                {"type": "input_image", "file_id": file_id},
            ],
        }
    ],
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using System.IO;
using System.Threading.Tasks;
using Azure.Identity;
using OpenAI;
using OpenAI.Files;
using OpenAI.Responses;
using System.ClientModel;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

OpenAIFileClient fileClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new OpenAIClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

byte[] imageBytes = await File.ReadAllBytesAsync("path_to_your_image.jpg");
OpenAIFile uploadedFile = await fileClient.UploadFileAsync(
    BinaryData.FromBytes(imageBytes),
    "path_to_your_image.jpg",
    FileUploadPurpose.Assistants);

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems =
    {
        ResponseItem.CreateUserMessageItem(
        [
            ResponseContentPart.CreateInputTextPart("What is in this image?"),
            ResponseContentPart.CreateInputImagePart(uploadedFile.Id)
        ])
    }
};

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import fs from "node:fs";
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const file = await client.files.create({
  file: fs.createReadStream("path_to_your_image.jpg"),
  purpose: "assistants",
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  input: [
    {
      role: "user",
      content: [
        { type: "input_text", text: "What is in this image?" },
        { type: "input_image", file_id: file.id },
      ],
    },
  ],
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.AzureApiKeyCredential;
import com.openai.models.files.FileCreateParams;
import com.openai.models.files.FileObject;
import com.openai.models.files.FilePurpose;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseInputImage;
import com.openai.models.responses.ResponseInputItem;
import java.nio.file.Paths;
import java.util.List;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

FileObject uploaded = openAIClient.files().create(
    FileCreateParams.builder()
        .file(Paths.get("path_to_your_image.jpg"))
        .purpose(FilePurpose.ASSISTANTS)
        .build());

ResponseInputImage image = ResponseInputImage.builder()
    .detail(ResponseInputImage.Detail.AUTO)
    .fileId(uploaded.id())
    .build();

ResponseInputItem userMsg = ResponseInputItem.ofMessage(
    ResponseInputItem.Message.builder()
        .role(ResponseInputItem.Message.Role.USER)
        .addInputTextContent("What is in this image?")
        .addContent(image)
        .build());

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .inputOfResponse(List.of(userMsg))
        .build());

System.out.println(response.outputText());
```

# [REST](#tab/rest)
```bash
# Upload the image
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/files \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -F purpose="assistants" \
  -F file="@path_to_your_image.jpg"

# Use the returned file ID with Responses
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": [
      {
        "role": "user",
        "content": [
          {"type": "input_text", "text": "What is in this image?"},
          {"type": "input_image", "file_id": "<file_id>"}
        ]
      }
    ]
  }'
```

---

### 图像输入要求

下表列出了图像输入支持的文件类型。

| 文件类型 | MIME 类型 |
| --- | --- |
| PNG | `image/png` |
| JPEG | `image/jpeg` |
| WebP | `image/webp` |
| 非动画 GIF | `image/gif` |

在单个请求中，最多可以包含 50 个图像。 每个单独的图像文件必须低于 50 MB，并且请求中所有映像的组合大小也必须低于 50 MB。

映像必须满足以下附加要求：

- 图像必须与提示相关;该模型不是针对不相关的视觉内容设计的。
- 图像不应包含违反内容策略的有害或敏感内容。
- 图像文件无法损坏或不可读。 如果模型无法处理映像，请求将失败。

### 选择图像详细信息级别

使用 `detail` 内容部件上的 `input_image` 属性来控制模型处理图像的方式。 较低的详细信息使用更少的令牌且速度更快，而更详细的详细信息使用更多令牌，但允许模型捕获更精细的功能。

```json
{
  "type": "input_image",
  "image_url": "<image_url>",
  "detail": "high"
}
```

下表描述了每个详细信息级别。

| 详细程度 | Description |
| --- | --- |
| `low` | 模型使用映像的分辨率较低版本。 此选项使用最少的令牌并生成最快的响应，但模型可能会错过精细的详细信息。 |
| `high` | 模型使用映像的更高分辨率版本。 此选项捕获更精细的详细信息，但使用更多令牌并花费更长的时间进行响应。 |
| `auto` | 默认值。 模型根据图像和提示选择适当的详细信息级别。 |

### 图像输入限制

已启用视觉的模型具有以下限制：

- **医学图像**：该模型不适合解释专门的医学图像，如CT扫描，不应用于医疗建议。
- **非英语文本**：处理包含非拉丁字母（如日语或朝鲜语）中的文本的图像时，模型可能无法以最佳方式执行。
- **小文本**：放大图像中的文本以提高可读性，但避免裁剪重要详细信息。
- **旋转**：模型可能错误地解释旋转或倒置的文本和图像。
- **视觉元素**：模型可能难以处理颜色或样式（如纯色、虚线或虚线）变化的图形或文本。
- **空间推理**：模型难以执行需要精确空间本地化的任务，例如识别国际象棋位置。
- **准确性**：在某些情况下，模型可能会生成不正确的说明或标题。
- **图像形状**：模型难以处理全景图像和鱼眼图像。
- **元数据和大小调整**：模型不会处理原始文件名或元数据，并且图像在分析之前调整大小，这会影响其原始尺寸。
- **计数**：模型可能会为图像中的对象提供近似计数。
- **CAPTCHA**：出于安全原因，系统会阻止提交 CAPTCHA。

## 文件输入

具有视觉功能的模型支持 PDF 输入。 PDF 文件可以作为 Base64 编码的数据或文件 ID 提供。 为了帮助模型解释 PDF 内容，提取的文本和每个页面的图像都包含在模型的上下文中。 当通过图表或非文本内容传达关键信息时，这非常有用。

注意

- 所有提取的文本和图像都放入模型的上下文中。 请确保了解将 PDF 用作输入的定价和令牌使用含义。
- 在单个 API 请求中，可以包含多个文件，但每个文件必须小于 50 MB。 请求中所有文件的组合限制为 50 MB。
- 只有支持文本和图像输入的模型才能接受 PDF 文件作为输入。
- 目前不支持 `purpose` 的 `user_data`。 作为临时解决方法，需要将 `assistants`目的设置为 。

### 将 PDF 转换为 Base64 并进行分析

通过将 PDF 字节编码为 base64 数据 URI 来内联发送 PDF。 模型同时接收提取的文本和每个页面的呈现图像。

# [Python](#tab/python)
```python
import base64
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

with open("PDF-FILE-NAME.pdf", "rb") as f:
    base64_string = base64.b64encode(f.read()).decode("utf-8")

response = client.responses.create(
    model="MODEL_NAME",
    input=[
        {
            "role": "user",
            "content": [
                {
                    "type": "input_file",
                    "filename": "PDF-FILE-NAME.pdf",
                    "file_data": f"data:application/pdf;base64,{base64_string}",
                },
                {"type": "input_text", "text": "Summarize this PDF."},
            ],
        },
    ]
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using System.IO;
using System.Threading.Tasks;
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

byte[] pdfBytes = await File.ReadAllBytesAsync("PDF-FILE-NAME.pdf");
BinaryData pdfData = BinaryData.FromBytes(pdfBytes);

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems =
    {
        ResponseItem.CreateUserMessageItem(
        [
            ResponseContentPart.CreateInputFilePart(pdfData, "application/pdf", "PDF-FILE-NAME.pdf"),
            ResponseContentPart.CreateInputTextPart("Summarize this PDF.")
        ])
    }
};

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import { readFileSync } from "node:fs";
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const base64Pdf = readFileSync("PDF-FILE-NAME.pdf").toString("base64");

const response = await client.responses.create({
  model: "MODEL_NAME",
  input: [
    {
      role: "user",
      content: [
        {
          type: "input_file",
          filename: "PDF-FILE-NAME.pdf",
          file_data: `data:application/pdf;base64,${base64Pdf}`,
        },
        { type: "input_text", text: "Summarize this PDF." },
      ],
    },
  ],
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseInputFile;
import com.openai.models.responses.ResponseInputItem;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.Base64;
import java.util.List;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

byte[] pdfBytes = Files.readAllBytes(Paths.get("document.pdf"));
String dataUrl = "data:application/pdf;base64," + Base64.getEncoder().encodeToString(pdfBytes);

ResponseInputFile file = ResponseInputFile.builder()
    .filename("document.pdf")
    .fileData(dataUrl)
    .build();

ResponseInputItem userMsg = ResponseInputItem.ofMessage(
    ResponseInputItem.Message.builder()
        .role(ResponseInputItem.Message.Role.USER)
        .addInputTextContent("Summarize this PDF.")
        .addContent(file)
        .build());

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .inputOfResponse(List.of(userMsg))
        .build());

System.out.println(response.outputText());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": [
      {
        "role": "user",
        "content": [
          {"type": "input_file", "filename": "PDF-FILE-NAME.pdf", "file_data": "data:application/pdf;base64,<BASE64_PDF>"},
          {"type": "input_text", "text": "Summarize this PDF."}
        ]
      }
    ]
  }'
```

---

### 上传 PDF 和分析

使用 `purpose="assistants"` 上传 PDF 文件。 目前不支持 `purpose` 中的 `user_data`。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

file = client.files.create(
    file=open("nucleus_sampling.pdf", "rb"),
    purpose="assistants"
)

response = client.responses.create(
    model="MODEL_NAME",
    input=[
        {
            "role": "user",
            "content": [
                {"type": "input_file", "file_id": file.id},
                {"type": "input_text", "text": "Summarize this PDF."},
            ],
        },
    ]
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using System.IO;
using System.Threading.Tasks;
using Azure.Identity;
using OpenAI;
using OpenAI.Files;
using OpenAI.Responses;
using System.ClientModel;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

OpenAIFileClient fileClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new OpenAIClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

byte[] pdfBytes = await File.ReadAllBytesAsync("nucleus_sampling.pdf");
OpenAIFile uploadedFile = await fileClient.UploadFileAsync(
    BinaryData.FromBytes(pdfBytes),
    "nucleus_sampling.pdf",
    FileUploadPurpose.Assistants);

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems =
    {
        ResponseItem.CreateUserMessageItem(
        [
            ResponseContentPart.CreateInputFilePart(uploadedFile.Id),
            ResponseContentPart.CreateInputTextPart("Summarize this PDF.")
        ])
    }
};

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import fs from "node:fs";
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const file = await client.files.create({
  file: fs.createReadStream("nucleus_sampling.pdf"),
  purpose: "assistants",
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  input: [
    {
      role: "user",
      content: [
        { type: "input_file", file_id: file.id },
        { type: "input_text", text: "Summarize this PDF." },
      ],
    },
  ],
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.files.FileCreateParams;
import com.openai.models.files.FileObject;
import com.openai.models.files.FilePurpose;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseInputFile;
import com.openai.models.responses.ResponseInputItem;
import java.nio.file.Paths;
import java.util.List;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

FileObject uploaded = openAIClient.files().create(
    FileCreateParams.builder()
        .file(Paths.get("document.pdf"))
        .purpose(FilePurpose.ASSISTANTS)
        .build());

ResponseInputFile file = ResponseInputFile.builder()
    .fileId(uploaded.id())
    .build();

ResponseInputItem userMsg = ResponseInputItem.ofMessage(
    ResponseInputItem.Message.builder()
        .role(ResponseInputItem.Message.Role.USER)
        .addInputTextContent("Summarize this PDF.")
        .addContent(file)
        .build());

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .inputOfResponse(List.of(userMsg))
        .build());

System.out.println(response.outputText());
```

# [REST](#tab/rest)
```bash
# Upload the PDF
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/files \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -F purpose="assistants" \
  -F file="@your_file.pdf"

# Use the returned file ID with Responses
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": [
      {
        "role": "user",
        "content": [
          {"type": "input_file", "file_id": "<file_id>"},
          {"type": "input_text", "text": "Summarize this PDF."}
        ]
      }
    ]
  }'
```

---

## 使用远程 MCP 服务器

可以通过将其连接到远程模型上下文协议 （MCP） 服务器上托管的工具来扩展模型的功能。 这些服务器由开发人员和组织维护，并公开可由 MCP 兼容的客户端（例如响应 API）访问的工具。

[模型上下文协议](https://modelcontextprotocol.io/introduction) （MCP）是一个开放标准，用于定义应用程序如何向大型语言模型（LLM）提供工具和上下文数据。 它支持将外部工具与模型工作流的一致、可缩放集成。

以下示例演示如何使用远程 MCP 服务器查询有关Azure REST API 存储库的信息。 模型实时检索存储库内容并基于其进行推理。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

response = client.responses.create(
    model="MODEL_NAME",
    tools=[
        {
            "type": "mcp",
            "server_label": "github",
            "server_url": "https://contoso.com/Azure/azure-rest-api-specs",
            "require_approval": "never"
        }
    ],
    input="What transport protocols are supported in the 2025-03-26 version of the MCP spec?"
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems = { ResponseItem.CreateUserMessageItem("What transport protocols are supported in the 2025-03-26 version of the MCP spec?") },
    Tools =
    {
        new McpTool(serverLabel: "github", serverUri: new Uri("https://contoso.com/Azure/azure-rest-api-specs"))
        {
            ToolCallApprovalPolicy = GlobalMcpToolCallApprovalPolicy.NeverRequireApproval
        }
    }
};

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  tools: [
    {
      type: "mcp",
      server_label: "github",
      server_url: "https://contoso.com/Azure/azure-rest-api-specs",
      require_approval: "never",
    },
  ],
  input: "What is this repo in 100 words?",
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.Tool;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Tool mcpTool = Tool.ofMcp(
    Tool.Mcp.builder()
        .serverLabel("github")
        .serverUrl("https://contoso.com/Azure/azure-rest-api-specs")
        .requireApproval(Tool.Mcp.RequireApproval.ofMcpToolApprovalSetting(
            Tool.Mcp.RequireApproval.McpToolApprovalSetting.NEVER))
        .build());

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .input("What is this repo in 100 words?")
        .addTool(mcpTool)
        .build());

System.out.println(response.outputText());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "tools": [
      {
        "type": "mcp",
        "server_label": "github",
        "server_url": "https://contoso.com/Azure/azure-rest-api-specs",
        "require_approval": "never"
      }
    ],
    "input": "What is this repo in 100 words?"
  }'
```

---

MCP 工具仅适用于响应 API，并且适用于所有较新的模型（gpt-4o、gpt-4.1 和推理模型）。 使用 MCP 工具时，只需为导入工具定义或进行工具调用时使用的令牌付费，无需额外付费。

### 批准

默认情况下，在与远程 MCP 服务器共享任何数据之前，响应 API 需要显式批准。 此审批步骤有助于确保透明度，并让你控制外部发送的信息。

建议查看与远程 MCP 服务器共享的所有数据，并选择性地记录这些数据以进行审核。

当需要审批时，模型在响应输出中返回一个 `mcp_approval_request` 项。 此对象包含挂起请求的详细信息，并允许在继续操作之前检查或修改数据。

```json
{
  "id": "mcpr_682bd9cd428c8198b170dc6b549d66fc016e86a03f4cc828",
  "type": "mcp_approval_request",
  "arguments": {},
  "name": "fetch_azure_rest_api_docs",
  "server_label": "github"
}
```

若要继续进行远程 MCP 调用，必须通过创建包含mcp\_approval\_response项的新响应对象来响应审批请求。 此对象确认了允许模型将指定数据发送到远程 MCP 服务器的意图。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

response = client.responses.create(
    model="MODEL_NAME",
    tools=[
        {
            "type": "mcp",
            "server_label": "github",
            "server_url": "https://contoso.com/Azure/azure-rest-api-specs",
            "require_approval": "never"
        }
    ],
    previous_response_id="<previous_response_id>",
    input=[
        {
            "type": "mcp_approval_response",
            "approve": True,
            "approval_request_id": "<approval_request_id>"
        }
    ]
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

McpTool mcpTool = new(serverLabel: "github", serverUri: new Uri("https://contoso.com/Azure/azure-rest-api-specs"));

ResponseResult priorResponse = await openAIClient.GetResponseAsync("<previous_response_id>");

foreach (ResponseItem item in priorResponse.OutputItems)
{
    if (item is McpToolCallApprovalRequestItem approvalRequest)
    {
        CreateResponseOptions followUp = new()
        {
            Model = "MODEL_NAME",
            PreviousResponseId = priorResponse.Id,
            InputItems = { new McpToolCallApprovalResponseItem(approvalRequest.Id, approved: true) },
            Tools = { mcpTool }
        };

        ResponseResult finalResponse = await openAIClient.CreateResponseAsync(followUp);
        Console.WriteLine(finalResponse.GetOutputText());
    }
}
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  tools: [
    {
      type: "mcp",
      server_label: "github",
      server_url: "https://contoso.com/Azure/azure-rest-api-specs",
      require_approval: "never",
    },
  ],
  previous_response_id: "<previous_response_id>",
  input: [
    {
      type: "mcp_approval_response",
      approve: true,
      approval_request_id: "<approval_request_id>",
    },
  ],
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseInputItem;
import java.util.List;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .previousResponseId("<previous_response_id>")
        .inputOfResponse(List.of(
            ResponseInputItem.ofMcpApprovalResponse(
                ResponseInputItem.McpApprovalResponse.builder()
                    .approvalRequestId("<approval_request_id>")
                    .approve(true)
                    .build())))
        .build());

System.out.println(response.outputText());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "tools": [
      {
        "type": "mcp",
        "server_label": "github",
        "server_url": "https://contoso.com/Azure/azure-rest-api-specs",
        "require_approval": "never"
      }
    ],
    "previous_response_id": "<previous_response_id>",
    "input": [
      {
        "type": "mcp_approval_response",
        "approve": true,
        "approval_request_id": "<approval_request_id>"
      }
    ]
  }'
```

---

### 认证

重要

- 响应 API 中的 MCP 客户端需要 TLS 1.2 或更高版本。
- 目前不支持双向 TLS（mTLS）。
- 目前，[Azure 服务标记](/zh-cn/azure/virtual-network/service-tags-overview) 不支持 MCP 客户端流量。

与GitHub MCP 服务器不同，大多数远程 MCP 服务器都需要身份验证。 响应 API 中的 MCP 工具支持自定义标头，允许你使用所需的身份验证方案安全地连接到这些服务器。

可以直接在请求中指定标头，例如 API 密钥、OAuth 访问令牌或其他凭据。 最常用的标头是 `Authorization` 标头。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

response = client.responses.create(
    model="MODEL_NAME",
    input="What is this repo in 100 words?",
    tools=[
        {
            "type": "mcp",
            "server_label": "github",
            "server_url": "https://contoso.com/Azure/azure-rest-api-specs",
            "headers": {"Authorization": "Bearer $YOUR_MCP_TOKEN"}
        }
    ]
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems = { ResponseItem.CreateUserMessageItem("What is this repo in 100 words?") },
    Tools =
    {
        new McpTool(serverLabel: "github", serverUri: new Uri("https://contoso.com/Azure/azure-rest-api-specs"))
        {
            AuthorizationToken = Environment.GetEnvironmentVariable("YOUR_MCP_TOKEN"),
            ToolCallApprovalPolicy = GlobalMcpToolCallApprovalPolicy.NeverRequireApproval
        }
    }
};

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  input: "What is this repo in 100 words?",
  tools: [
    {
      type: "mcp",
      server_label: "github",
      server_url: "https://contoso.com/Azure/azure-rest-api-specs",
      headers: { Authorization: "Bearer $YOUR_MCP_TOKEN" },
    },
  ],
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.core.JsonValue;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.Tool;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Tool mcpTool = Tool.ofMcp(
    Tool.Mcp.builder()
        .serverLabel("github")
        .serverUrl("https://contoso.com/Azure/azure-rest-api-specs")
        .headers(Tool.Mcp.Headers.builder()
            .putAdditionalProperty("Authorization", JsonValue.from("Bearer $YOUR_MCP_TOKEN"))
            .build())
        .requireApproval(Tool.Mcp.RequireApproval.ofMcpToolApprovalSetting(
            Tool.Mcp.RequireApproval.McpToolApprovalSetting.NEVER))
        .build());

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .input("What is this repo in 100 words?")
        .addTool(mcpTool)
        .build());

System.out.println(response.outputText());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": "What is this repo in 100 words?",
    "tools": [
      {
        "type": "mcp",
        "server_label": "github",
        "server_url": "https://contoso.com/Azure/azure-rest-api-specs",
        "headers": {"Authorization": "Bearer $YOUR_MCP_TOKEN"}
      }
    ]
  }'
```

---

## 后台任务

后台模式可让你借助 `o3` 和 `o1-pro` 等推理模型，以异步方式运行长时间运行的任务。 对于可能需要几分钟才能完成的复杂任务（例如 Codex 或 Deep Research 样式代理）非常有用。 当使用 `"background": true` 发送请求时，任务将异步处理，您需要轮询其状态。

### 启动后台任务

在请求中设置 `background=true` 以将任务加入队列。 该服务会立即返回响应 ID 和 `queued` 状态 — 使用该 ID 轮询、流式传输或取消任务。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

response = client.responses.create(
    model="MODEL_NAME",
    input="Write me a very long story.",
    background=True
)

print(response.status)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems = { ResponseItem.CreateUserMessageItem("Write me a very long story.") },
    BackgroundModeEnabled = true
};

ResponseResult queued = await openAIClient.CreateResponseAsync(options);
Console.WriteLine($"Response id: {queued.Id}");
Console.WriteLine($"Status: {queued.Status}");
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  input: "Write me a very long story.",
  background: true,
});

console.log(response.status);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .input("Write a 1000-word essay on the history of computing.")
        .background(true)
        .build());

System.out.println(response.status());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": "Write me a very long story.",
    "background": true
  }'
```

---

### 轮询完成状态

当状态为 `queued` 或 `in_progress` 时，继续轮询。 响应到达终端状态后，即可进行检索。

# [Python](#tab/python)
```python
from time import sleep

while response.status in {"queued", "in_progress"}:
    print(f"Current status: {response.status}")
    sleep(2)
    response = client.responses.retrieve(response.id)

print(f"Final status: {response.status}\nOutput:\n{response.output_text}")
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using System.Threading.Tasks;
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

ResponseResult current = await openAIClient.GetResponseAsync("<response_id>");

while (current.Status == ResponseStatus.Queued || current.Status == ResponseStatus.InProgress)
{
    Console.WriteLine($"Current status: {current.Status}");
    await Task.Delay(TimeSpan.FromSeconds(2));
    current = await openAIClient.GetResponseAsync(current.Id);
}

Console.WriteLine($"Final status: {current.Status}");
if (current.Status == ResponseStatus.Completed)
{
    Console.WriteLine(current.GetOutputText());
}
```

# [JavaScript](#tab/javascript)
```javascript
let current = response;
while (current.status === "queued" || current.status === "in_progress") {
  console.log(`Current status: ${current.status}`);
  await new Promise((r) => setTimeout(r, 2000));
  current = await client.responses.retrieve(current.id);
}
console.log(`Final status: ${current.status}\nOutput:\n${current.output_text}`);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Response current = openAIClient.responses().retrieve("<response_id>");
while (current.status().filter(s ->
        s.equals(Response.Status.QUEUED) || s.equals(Response.Status.IN_PROGRESS)).isPresent()) {
    System.out.println("Current status: " + current.status());
    Thread.sleep(2000);
    current = openAIClient.responses().retrieve(current.id());
}
System.out.println("Final status: " + current.status());
System.out.println("Output:\n" + current.outputText());
```

# [REST](#tab/rest)
```bash
curl -X GET https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses/<response_id> \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY"
```

---

### 取消后台任务

使用 `cancel` 端点取消正在进行中的后台任务。 取消是幂等的 - 后续调用返回最终响应对象。

# [Python](#tab/python)
```python
response = client.responses.cancel("<response_id>")
print(response.status)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

ResponseResult cancelled = await openAIClient.CancelResponseAsync("<response_id>");
Console.WriteLine($"Status: {cancelled.Status}");
```

# [JavaScript](#tab/javascript)
```javascript
const cancelled = await client.responses.cancel("<response_id>");
console.log(cancelled.status);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Response cancelled = openAIClient.responses().cancel("<response_id>");
System.out.println(cancelled.status());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses/<response_id>/cancel \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY"
```

---

若要流式传输后台响应，请将 `background` 和 `stream` 均设置为 `true`。 这种模式可让你在连接中断后继续进行流式传输。 通过每个事件中的 `sequence_number` 跟踪你的位置。

# [Python](#tab/python)
```python
stream = client.responses.create(
    model="MODEL_NAME",
    input="Write me a very long story.",
    background=True,
    stream=True,
)

cursor = None
for event in stream:
    print(event)
    cursor = event["sequence_number"]
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions createOptions = new()
{
    Model = "MODEL_NAME",
    InputItems = { ResponseItem.CreateUserMessageItem("Write me a very long story.") },
    BackgroundModeEnabled = true,
    StreamingEnabled = true
};

string queuedResponseId = null;
int lastSequenceNumber = 0;

await foreach (StreamingResponseUpdate update in openAIClient.CreateResponseStreamingAsync(createOptions))
{
    if (update is StreamingResponseQueuedUpdate queuedUpdate)
    {
        queuedResponseId = queuedUpdate.Response.Id;
        lastSequenceNumber = queuedUpdate.SequenceNumber;
        Console.WriteLine($"Queued response: {queuedResponseId}, sequence {lastSequenceNumber}");
        break;
    }
}

// Resume streaming from where we disconnected.
GetResponseOptions resumeOptions = new(queuedResponseId)
{
    StartingAfter = lastSequenceNumber,
    StreamingEnabled = true
};

await foreach (StreamingResponseUpdate update in openAIClient.GetResponseStreamingAsync(resumeOptions))
{
    Console.WriteLine(update.GetType().Name);
    if (update is StreamingResponseCompletedUpdate completed)
    {
        Console.WriteLine($"[done] final id: {completed.Response.Id}");
    }
}
```

# [JavaScript](#tab/javascript)
```javascript
const stream = await client.responses.create({
  model: "MODEL_NAME",
  input: "Write me a very long story.",
  background: true,
  stream: true,
});

let cursor = null;
for await (const event of stream) {
  console.log(event);
  cursor = event.sequence_number;
}
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.core.http.StreamResponse;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.ResponseRetrieveParams;
import com.openai.models.responses.ResponseStreamEvent;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

long cursor = 0L;
try (StreamResponse<ResponseStreamEvent> stream = openAIClient.responses().retrieveStreaming(
        "<response_id>",
        ResponseRetrieveParams.builder().startingAfter(cursor).build())) {
    stream.stream().forEach(event -> System.out.println(event));
}
```

# [REST](#tab/rest)
```bash
curl -N -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "input": "Write me a very long story.",
    "background": true,
    "stream": true
  }'
```

---

后台响应当前比同步响应具有更高的首个令牌时间。 正在改进，以减少这种差距。

### 限制

- 后台模式需要 `store=true`。 不支持无状态请求。
- 仅当原始请求包含 `stream=true`时，您才能重新开始流式传输。
- 若要取消同步响应，请直接终止连接。

### 从特定点恢复流式传输

如果流式连接中断，您可以通过在响应中传递 `stream=true` 以及 `starting_after=<sequence_number>` 上的 `GET`，从已知事件处恢复。 服务会重播该序列号之后发布的事件。

```bash
curl -N -X GET "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses/<response_id>?stream=true&starting_after=42" \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY"
```

## 加密推理项

当你在无状态模式（`store=false`）下使用 Responses API 时，仍然必须在各个对话轮次之间保留推理上下文。 为此，请在请求中包含加密的推理项。

若要在多轮对话中保留推理项，请将 `reasoning.encrypted_content` 添加到 `include` 参数中。 然后，响应包含推理跟踪的加密版本，可以传递给将来的请求。

# [Python](#tab/python)
```python
import os
from openai import OpenAI

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=os.getenv("AZURE_OPENAI_API_KEY")
)

response = client.responses.create(
    model="MODEL_NAME",
    reasoning={"effort": "medium"},
    input="What is the weather like today?",
    tools=[
        # Replace with your function or tool definitions.
    ],
    include=["reasoning.encrypted_content"],
    store=False,
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using System.Collections.Generic;
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

List<ResponseItem> inputItems =
[
    ResponseItem.CreateUserMessageItem("<your_prompt>")
];

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    StoredOutputEnabled = false,
    IncludedProperties = { IncludedResponseProperty.ReasoningEncryptedContent }
};
foreach (ResponseItem item in inputItems)
{
    options.InputItems.Add(item);
}

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());

// To carry encrypted reasoning into a follow-up turn, append response.OutputItems to inputItems
// and resend with StoredOutputEnabled = false. Don't use PreviousResponseId when not stored.
```

# [JavaScript](#tab/javascript)
```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  reasoning: { effort: "medium" },
  input: "What is the weather like today?",
  tools: [
    // Replace with your function or tool definitions.
  ],
  include: ["reasoning.encrypted_content"],
  store: false,
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Reasoning;
import com.openai.models.responses.ReasoningEffort;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseIncludable;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .input("Explain quantum entanglement.")
        .reasoning(Reasoning.builder().effort(ReasoningEffort.MEDIUM).build())
        .addInclude(ResponseIncludable.REASONING_ENCRYPTED_CONTENT)
        .store(false)
        .build());

System.out.println(response.outputText());
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
    "model": "MODEL_NAME",
    "reasoning": {"effort": "medium"},
    "input": "What is the weather like today?",
    "tools": [],
    "include": ["reasoning.encrypted_content"],
    "store": false
  }'
```

---

响应 API 支持将图像生成作为对话和多步骤工作流的一部分。 它支持上下文中的图像输入和输出，并包括用于生成和编辑图像的内置工具。

与独立映像 API 相比，响应 API 提供两个优势：

- **流式处理**：在生成过程中显示部分图像输出以提高感知延迟。
- **灵活的输入**：除了原始图像字节外，还接受图像文件 ID 作为输入。

注意

响应 API 中的图像生成工具受 `gpt-image-1`系列模型支持，可以从一组兼容的聊天和推理模型调用它。 有关支持的业务流程模型的当前列表，请参阅本文后面的 “支持的模型 ”部分。

图像生成工具目前不支持流式处理模式。 若要流式传输部分图像，请直接在响应 API 外部调用 [图像生成 API](dall-e) 。

使用响应 API 通过 GPT 图像模型生成对话图像体验。

# [Python](#tab/python)
```python
import base64
import os
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

token_provider = get_bearer_token_provider(
    DefaultAzureCredential(), "https://ai.azure.com/.default"
)

client = OpenAI(
    base_url="https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
    api_key=token_provider,
    default_headers={
        "x-ms-oai-image-generation-deployment": os.getenv("IMAGE_MODEL_NAME"),
        "api_version": "preview",
    },
)

response = client.responses.create(
    model="MODEL_NAME",
    input="Generate an image of a gray tabby cat hugging an otter with an orange scarf.",
    tools=[{"type": "image_generation"}],
)

image_data = [
    output.result
    for output in response.output
    if output.type == "image_generation_call"
]

if image_data:
    with open("otter.png", "wb") as f:
        f.write(base64.b64decode(image_data[0]))
```

# [C#](#tab/csharp)
```csharp
#pragma warning disable OPENAI001
using System.IO;
using System.Threading.Tasks;
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

// API key authentication
ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

// Microsoft Entra ID authentication (recommended)
BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");
ResponsesClient openAIClientEntra = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

ImageGenerationTool imageTool = ResponseTool.CreateImageGenerationTool(model: "gpt-image-1");

CreateResponseOptions options = new()
{
    Model = "MODEL_NAME",
    InputItems =
    {
        ResponseItem.CreateUserMessageItem("Generate an image of an otter swimming in a pond.")
    },
    Tools = { imageTool }
};

ResponseResult response = await openAIClient.CreateResponseAsync(options);

foreach (ResponseItem item in response.OutputItems)
{
    if (item is ImageGenerationCallResponseItem imageCall && imageCall.ImageResultBytes is not null)
    {
        await File.WriteAllBytesAsync("otter.png", imageCall.ImageResultBytes.ToArray());
        Console.WriteLine($"Saved image. Revised prompt: {imageCall.RevisedPrompt}");
    }
}
```

# [JavaScript](#tab/javascript)
```javascript
import fs from "fs";
import OpenAI from "openai";
import { DefaultAzureCredential, getBearerTokenProvider } from "@azure/identity";

const tokenProvider = getBearerTokenProvider(
  new DefaultAzureCredential(),
  "https://ai.azure.com/.default"
);

const client = new OpenAI({
  baseURL: "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/",
  apiKey: await tokenProvider(),
  defaultHeaders: {
    "x-ms-oai-image-generation-deployment": process.env.IMAGE_MODEL_NAME,
    api_version: "preview",
  },
});

const response = await client.responses.create({
  model: "MODEL_NAME",
  input: "Generate an image of a gray tabby cat hugging an otter with an orange scarf.",
  tools: [{ type: "image_generation" }],
});

const imageBase64 = response.output
  .filter((o) => o.type === "image_generation_call")
  .map((o) => o.result)[0];

if (imageBase64) {
  fs.writeFileSync("otter.png", Buffer.from(imageBase64, "base64"));
}
```

# [Java](#tab/java)
```java
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.azure.identity.AuthenticationUtil;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseOutputItem;
import com.openai.models.responses.Tool;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.Base64;

String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
    .baseUrl(endpoint)
    .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
    .build();

Tool imageGen = Tool.ofImageGeneration(Tool.ImageGeneration.builder().build());

Response response = openAIClient.responses().create(
    ResponseCreateParams.builder()
        .model("MODEL_NAME")
        .input("Generate an image of a gray tabby cat hugging an otter with an orange scarf.")
        .addTool(imageGen)
        .build());

response.output().stream()
    .filter(ResponseOutputItem::isImageGenerationCall)
    .map(ResponseOutputItem::asImageGenerationCall)
    .findFirst()
    .flatMap(call -> call.result())
    .ifPresent(b64 -> {
        try {
            Files.write(Paths.get("otter.png"), Base64.getDecoder().decode(b64));
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    });
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -H "x-ms-oai-image-generation-deployment: $IMAGE_MODEL_NAME" \
  -d '{
    "model": "MODEL_NAME",
    "input": "Generate an image of a gray tabby cat hugging an otter with an orange scarf.",
    "tools": [{ "type": "image_generation" }]
  }'
```

---

## 推理模型

有关如何将推理模型与响应 API 配合使用的示例，请参阅 [推理模型指南](reasoning#reasoning-summary)。

## 计算机使用

Playwright 的计算使用已迁移至[专用计算使用模型指南](../../../foundry-classic/openai/how-to/computer-use#playwright-integration)。

## 故障 排除

- **401/403**：如果使用 Microsoft Entra ID，请验证是否针对 `https://ai.azure.com/.default` 确定了令牌范围。 如果使用 API 密钥，请确认你使用的是资源的正确密钥。
- **404**：确认 `model` 与部署名称匹配。
