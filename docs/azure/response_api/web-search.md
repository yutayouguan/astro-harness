---
layout: Conceptual
title: 使用响应 API 进行 Web 搜索 - Microsoft Foundry | Microsoft Learn
canonicalUrl: https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/web-search
schema: Conceptual
ai-usage: ai-assisted
author: alvinashcraft
breadcrumb_path: ../../../breadcrumb/azure-ai/toc.json
depot_name: Learn.azure-ai
description: 了解如何将 Web 搜索与响应 API 配合使用
feedback_help_link_type: get-help-at-qna
feedback_help_link_url: https://learn.microsoft.com/answers/tags/133/azure
feedback_product_url: https://feedback.azure.com/d365community/forum/79b1327d-d925-ec11-b6e6-000d3a4f06a4
feedback_system: Standard
git_commit_id: 777f84fbedeb0995f4fdea6c65bee2f2a4af1657
gitcommit: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/777f84fbedeb0995f4fdea6c65bee2f2a4af1657/articles/foundry/openai/how-to/web-search.md
learn_banner_products:
- azure
locale: zh-cn
manager: mcleans
ms.author: aashcraft
ms.collection: ce-skilling-ai-copilot
ms.custom:
- classic-and-new
- doc-kit-assisted
ms.date: 2026-05-13T00:00:00.0000000Z
ms.service: microsoft-foundry
ms.subservice: foundry-openai
ms.suite: office
ms.topic: how-to
ms.update-cycle: 90-days
original_content_git_url: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/live/articles/foundry/openai/how-to/web-search.md
permissioned-type: public
recommendation_types:
- Training
- Certification
recommendations: true
site_name: Docs
uhfHeaderId: azure-ai-foundry
updated_at: 2026-08-18T14:54:00.0000000Z
ms.translationtype: MT
ms.contentlocale: zh-cn
loc_version: 2026-06-12T17:37:06.2128336Z
loc_source_id: Github-845157915#live
loc_file_id: Github-845157915.live.Learn.azure-ai.articles/foundry/openai/how-to/web-search.md
page_type: conceptual
toc_rel: ../../toc.json
word_count: 3712
asset_id: foundry/openai/how-to/web-search
item_type: Content
platformId: cb26d69d-09e4-0130-9d45-5b1655f790dc
---

# 使用响应 API 进行 Web 搜索 - Microsoft Foundry | Microsoft Learn

Web 搜索使模型能够在生成输出之前，通过从公共 Web 中检索实时信息来验证响应。 启用后，模型可以使用内联引文返回最新的答案。 可以通过`web_search` 中的工具访问 Web 搜索。

注意

在 Azure OpenAI 响应 API 中，使用 `web_search` 工具进行 Web 搜索。 支持网络搜索工具（`web_search_preview`）的预览版本，但不建议使用。 它具有限制。

重要

- Web 搜索使用 Grounding with Bing Search 和/或 Grounding with Bing Custom Search，这些是受 [Grounding with Bing 使用条款](https://www.microsoft.com/licensing/terms/product/ForOnlineServices/EAEAS)和 [Microsoft 隐私声明](https://www.microsoft.com/en-us/bing/apis/grounding-legal-enterprise)约束的[第一方消费服务](https://go.microsoft.com/fwlink/?LinkId=521839&amp;clcid=0x409)。
- Microsoft [数据保护附录](https://aka.ms/dpa) 不适用于通过必应搜索和/或必应自定义搜索发送到 Grounding 的数据。 当您使用 Grounding with Bing Search 和/或 Grounding with Bing Custom Search 时，您的数据会流出您的合规和地理边界。
- 使用 Grounding with Bing Search 和 Grounding with Bing Custom Search 会产生费用。 若要了解详细信息，请参阅 [定价](https://www.microsoft.com/en-us/bing/apis)。
- 了解详细信息关于 Azure 管理员如何管理对 Web 搜索功能的访问权限。

## 先决条件

- 已部署Azure OpenAI 模型。
- 身份验证方法：
    - API 密钥，或
    - Microsoft Entra ID。
- 安装适用于语言的客户端库：
    - **Python**： `pip install openai azure-identity`
    - **.NET**： `dotnet add package OpenAI` 和 `dotnet add package Azure.Identity`
    - **JavaScript/TypeScript**： `npm install openai @azure/identity`
    - **Java**：向项目添加 `com.openai:openai-java` 和 `com.azure:azure-identity`。
- 对于 REST 示例，请设置 `AZURE_OPENAI_API_KEY` （API 密钥流）或 `AZURE_OPENAI_AUTH_TOKEN` （Microsoft Entra ID 流）。

## 使用 Web 搜索的选项

Web 搜索支持三种模式。 根据所需的深度和速度选择模式。

### 没有推理的 Web 搜索

该模型将用户查询直接转发到 Web 搜索工具，并使用排名较高的来源作为响应的依据。 没有多步骤规划。 此模式 **快速** 且最适合快速查找和及时事实。

### 使用推理模型进行代理搜索

该模型主动管理搜索过程，并且可以在其思维链中执行 Web 搜索、分析结果并确定是否继续搜索。 这种灵活性使代理搜索非常适合 **复杂的工作流**，但也意味着搜索花费 **的时间比** 快速查找更长。 例如，使用`gpt-5.5`，并将`reasoning.effort`设置为`medium`或`high`，以平衡搜索深度和延迟。

### 深入研究

深度研究是一种代理驱动的模式，专为 **扩展调查**而设计。 该模型会进行多步推理，打开并阅读多个页面，并将所得发现整合为内容全面、引用丰富的回答。 将此模式与 `o3-deep-research` 一起使用，或者将 `gpt-5.5` 和 `reasoning.effort` 设为 `high` 或 `xhigh`。

深度研究可以运行几分钟，最适合将完成度优先于速度的后台任务。

## 工作原理

通过在请求中声明该工具 `{"type": "web_search"}` 来使用 Web 搜索。 模型决定是否基于用户的提示和配置调用该工具。

注意

响应 API 中的 Web 搜索适用于 GPT-4 模型及更高版本。

在以下示例中，请将 `gpt-5.5` 替换为您自己的模型部署名称。 基本Microsoft Entra ID代码片段中显示的相同 `assert` 模式适用于 API 键代码段和用户位置和域筛选示例。

# [Python](#tab/python)
**Microsoft Entra ID：**

```python
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/"
token_provider = get_bearer_token_provider(
    DefaultAzureCredential(), "https://ai.azure.com/.default"
)

# Use the OpenAI client with the Azure v1 endpoint.
openai = OpenAI(
    base_url=endpoint,
    api_key=token_provider,
)

response = openai.responses.create(
    model="gpt-5.5",
    tools=[{"type": "web_search"}],
    input="Please perform a web search on the latest trends in renewable energy",
)

print(response.output_text)

# Verify the call succeeded.
assert response.output_text, "Empty output_text"
assert any(item.type == "web_search_call" for item in response.output), \
    "No web_search_call in response"
```

**API 密钥：**

```python
import os
from openai import OpenAI

endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/"

openai = OpenAI(
    base_url=endpoint,
    api_key=os.getenv("AZURE_OPENAI_API_KEY"),
)

response = openai.responses.create(
    model="gpt-5.5",
    tools=[{"type": "web_search"}],
    input="Please perform a web search on the latest trends in renewable energy",
)

print(response.output_text)
```

# [C#](#tab/csharp)
**Microsoft Entra ID：**

```csharp
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;
using System.Diagnostics;

#pragma warning disable OPENAI001

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");

ResponsesClient openAIClient = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions options = new()
{
    Model = "gpt-5.5",
    Tools = { ResponseTool.CreateWebSearchTool() }
};
options.InputItems.Add(ResponseItem.CreateUserMessageItem(
    "Please perform a web search on the latest trends in renewable energy"));

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());

// Verify the call succeeded.
Debug.Assert(!string.IsNullOrEmpty(response.GetOutputText()), "Empty output text");
Debug.Assert(
    response.OutputItems.Any(item => item is WebSearchCallResponseItem),
    "No web_search_call in response");
```

**API 密钥：**

```csharp
using OpenAI.Responses;
using System.ClientModel;

#pragma warning disable OPENAI001

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";
string apiKey = Environment.GetEnvironmentVariable("AZURE_OPENAI_API_KEY")!;

ResponsesClient openAIClient = new(
    credential: new ApiKeyCredential(apiKey),
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

CreateResponseOptions options = new()
{
    Model = "gpt-5.5",
    Tools = { ResponseTool.CreateWebSearchTool() }
};
options.InputItems.Add(ResponseItem.CreateUserMessageItem(
    "Please perform a web search on the latest trends in renewable energy"));

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
**Microsoft Entra ID：**

```javascript
// Save this file with the .mjs extension, or add "type": "module" to your package.json.
import { OpenAI } from "openai";
import { DefaultAzureCredential, getBearerTokenProvider } from "@azure/identity";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";
const tokenProvider = getBearerTokenProvider(
  new DefaultAzureCredential(),
  "https://ai.azure.com/.default"
);

const openai = new OpenAI({
  baseURL: endpoint,
  apiKey: await tokenProvider(),
});

const response = await openai.responses.create({
  model: "gpt-5.5",
  tools: [{ type: "web_search" }],
  input: "Please perform a web search on the latest trends in renewable energy",
});

console.log(response.output_text);

// Verify the call succeeded.
console.assert(response.output_text, "Empty output_text");
console.assert(
  response.output.some((item) => item.type === "web_search_call"),
  "No web_search_call in response"
);
```

**API 密钥：**

```javascript
import { OpenAI } from "openai";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";

const openai = new OpenAI({
  baseURL: endpoint,
  apiKey: process.env.AZURE_OPENAI_API_KEY,
});

const response = await openai.responses.create({
  model: "gpt-5.5",
  tools: [{ type: "web_search" }],
  input: "Please perform a web search on the latest trends in renewable energy",
});

console.log(response.output_text);
```

# [Java](#tab/java)
**Microsoft Entra ID：**

```java
import com.azure.identity.AuthenticationUtil;
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.WebSearchTool;

public class WebSearchExample {
    public static void main(String[] args) {
        String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

        OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
            .baseUrl(endpoint)
            .credential(BearerTokenCredential.create(
                AuthenticationUtil.getBearerTokenSupplier(
                    new DefaultAzureCredentialBuilder().build(),
                    "https://ai.azure.com/.default")))
            .build();

        WebSearchTool webSearchTool = WebSearchTool.builder()
            .type(WebSearchTool.Type.WEB_SEARCH)
            .build();

        ResponseCreateParams params = ResponseCreateParams.builder()
            .model("gpt-5.5")
            .input("Please perform a web search on the latest trends in renewable energy")
            .addTool(webSearchTool)
            .build();

        Response response = openAIClient.responses().create(params);
        response.output().forEach(item -> item.message().ifPresent(msg ->
            msg.content().forEach(content -> content.outputText().ifPresent(
                t -> System.out.println(t.text())))));

        // Verify the call succeeded.
        boolean hasWebSearchCall = response.output().stream()
            .anyMatch(item -> item.webSearchCall().isPresent());
        assert hasWebSearchCall : "No web_search_call in response";
    }
}
```

**API 密钥：**

```java
import com.openai.azure.credential.AzureApiKeyCredential;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.WebSearchTool;

public class WebSearchExample {
    public static void main(String[] args) {
        String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

        OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
            .baseUrl(endpoint)
            .credential(AzureApiKeyCredential.create(System.getenv("AZURE_OPENAI_API_KEY")))
            .build();

        WebSearchTool webSearchTool = WebSearchTool.builder()
            .type(WebSearchTool.Type.WEB_SEARCH)
            .build();

        ResponseCreateParams params = ResponseCreateParams.builder()
            .model("gpt-5.5")
            .input("Please perform a web search on the latest trends in renewable energy")
            .addTool(webSearchTool)
            .build();

        Response response = openAIClient.responses().create(params);
        response.output().forEach(item -> item.message().ifPresent(msg ->
            msg.content().forEach(content -> content.outputText().ifPresent(
                t -> System.out.println(t.text())))));
    }
}
```

# [REST](#tab/rest)
**Microsoft Entra ID：**

```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AZURE_OPENAI_AUTH_TOKEN" \
  -d '{
     "model": "gpt-5.5",
     "tools": [{"type": "web_search"}],
     "input": "Please perform a web search on the latest trends in renewable energy"
    }'
```

响应返回 HTTP 200，其中包含一个 JSON 正文，其 `output` 数组包含一个 `web_search_call` 项和一个 `message` 项。 如果缺少任一项，则调用未执行 Web 搜索。

**API 密钥：**

```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "api-key: $AZURE_OPENAI_API_KEY" \
  -d '{
     "model": "gpt-5.5",
     "tools": [{"type": "web_search"}],
     "input": "Please perform a web search on the latest trends in renewable energy"
    }'
```

---

### 响应形状

使用 Web 搜索的成功响应通常包含两个部分：

```json
[
    {
      "id": "ws_68b9d1220b288199bf942a3e48055f3602e3b78a8dbf73ac",
      "type": "web_search_call",
      "status": "completed",
      "action": {
        "type": "search",
        "query": "latest trends in renewable energy 2025"
      }
    },
    {
      "id": "msg_68b9d123f4788199a544b6b97e65673e02e3b78a8dbf73ac",
      "type": "message",
      "status": "completed",
      "role": "assistant",
      "content": [
        {
          "type": "output_text",
          "annotations": [
            {
                "type": "url_citation",
                "start_index": 1358,
                "end_index": 1462,
                "url": "https://...",
                "title": "Title..."
            }
        ],
          "text": "If you're searching for uplifting....."
        }
      ],
    }
  ]
```

- 记录执行的操作的一个`web_search_call`输出项：
    - `search`：一个网络搜索操作，包括查询（和可选的搜索域）。 **搜索操作会产生工具调用成本** （请参阅 [定价](https://www.microsoft.com/en-us/bing/apis)）。
    - `open_page`：指示代理打开了页面。 可用于所有推理模型。
    - `find_in_page`：表示代理在已打开的页面内进行了搜索。 可用于所有推理模型。
- 一个包含以下内容的消息输出项：
    - `message.content[0].text` 中的加粗文本。
    - `message.content[0].annotations` 中的 URL 引文，以及一个或多个包含 URL、标题和字符范围的 `url_citation` 对象。

使用推理模型时，`output` 数组除了 `reasoning` 和 `web_search_call` 之外，还包含一个 `message` 项。 通过 `type` 解析数组，而不是按位置解析。

### 按用户位置控制结果

可以通过传递近似用户位置来优化搜索结果。 支持以下字段：

| 领域 | Description | 例子 |
| --- | --- | --- |
| `country` | 双字母 [ISO 国家/地区代码](https://en.wikipedia.org/wiki/ISO_3166-1)。 | `US` |
| `city` | 自由文本城市名称。 | `Chicago` |
| `region` | 自由输入的地区或州名称。 | `Illinois` |
| `timezone` | [IANA 时区标识符](https://www.iana.org/time-zones)。 | `America/Chicago` |

注意

仅当使用 `city` 工具时，才支持 `region`、`timezone` 和 `web_search` 字段。

# [Python](#tab/python)
```python
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/"
token_provider = get_bearer_token_provider(
    DefaultAzureCredential(), "https://ai.azure.com/.default"
)

openai = OpenAI(base_url=endpoint, api_key=token_provider)

response = openai.responses.create(
    model="gpt-5.5",
    tools=[
        {
            "type": "web_search",
            "user_location": {
                "type": "approximate",
                "country": "US",
                "city": "Chicago",
                "region": "Illinois",
                "timezone": "America/Chicago",
            },
        }
    ],
    input="Give me a positive news story from the web today in my city",
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

#pragma warning disable OPENAI001

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");

ResponsesClient openAIClient = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

WebSearchToolApproximateLocation location =
    WebSearchToolLocation.CreateApproximateLocation(
        country: "US",
        region: "Illinois",
        city: "Chicago",
        timezone: "America/Chicago");

CreateResponseOptions options = new()
{
    Model = "gpt-5.5",
    Tools = { ResponseTool.CreateWebSearchTool(userLocation: location) }
};
options.InputItems.Add(ResponseItem.CreateUserMessageItem(
    "Give me a positive news story from the web today in my city"));

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import { OpenAI } from "openai";
import { DefaultAzureCredential, getBearerTokenProvider } from "@azure/identity";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";
const tokenProvider = getBearerTokenProvider(
  new DefaultAzureCredential(),
  "https://ai.azure.com/.default"
);

const openai = new OpenAI({
  baseURL: endpoint,
  apiKey: await tokenProvider(),
});

const response = await openai.responses.create({
  model: "gpt-5.5",
  tools: [
    {
      type: "web_search",
      user_location: {
        type: "approximate",
        country: "US",
        city: "Chicago",
        region: "Illinois",
        timezone: "America/Chicago",
      },
    },
  ],
  input: "Give me a positive news story from the web today in my city",
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.AuthenticationUtil;
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.WebSearchTool;
import com.openai.models.responses.WebSearchTool.UserLocation;

public class WebSearchLocationExample {
    public static void main(String[] args) {
        String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

        OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
            .baseUrl(endpoint)
            .credential(BearerTokenCredential.create(
                AuthenticationUtil.getBearerTokenSupplier(
                    new DefaultAzureCredentialBuilder().build(),
                    "https://ai.azure.com/.default")))
            .build();

        WebSearchTool webSearchTool = WebSearchTool.builder()
            .type(WebSearchTool.Type.WEB_SEARCH)
            .userLocation(UserLocation.builder()
                .country("US")
                .city("Chicago")
                .region("Illinois")
                .timezone("America/Chicago")
                .build())
            .build();

        ResponseCreateParams params = ResponseCreateParams.builder()
            .model("gpt-5.5")
            .input("Give me a positive news story from the web today in my city")
            .addTool(webSearchTool)
            .build();

        Response response = openAIClient.responses().create(params);
        response.output().forEach(item -> item.message().ifPresent(msg ->
            msg.content().forEach(content -> content.outputText().ifPresent(
                t -> System.out.println(t.text())))));
    }
}
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AZURE_OPENAI_AUTH_TOKEN" \
  -d '{
    "model": "gpt-5.5",
    "tools": [
        {
            "type": "web_search",
            "user_location": {
                "type": "approximate",
                "country": "US",
                "city": "Chicago",
                "region": "Illinois",
                "timezone": "America/Chicago"
            }
        }
    ],
    "input": "Give me a positive news story from the web today in my city"
    }'
```

---

若要使用 API 密钥身份验证，请将Microsoft Entra凭据替换为 API 密钥，如 basic 示例所示。

### 域名筛选

可以使用域筛选将结果限制为一组特定的域。 使用 `allowed_domains` 字段可将最多 100 个 URL 列入允许名单，或使用 `blocked_domains` 字段将域名从结果中排除。 设置 URL 格式时，可以省略 HTTP 或 HTTPS 前缀。 例如，请使用 `microsoft.com` 而不是 `https://www.microsoft.com/`。 子域也包含在搜索中。 域筛选仅在 Responses API 中与 `web_search` 工具配合使用。

注意

OpenAI .NET和Java SDK 尚未公开`blocked_domains`的类型化生成器。 这里的 C# 和 Java 示例使用 SDK 的公开接口（.NET 使用 `JsonPatch.Set`，Java 使用 `putAdditionalProperty`）来设置该字段。 当类型化构建器发布后，请将这些调用替换为对应的类型化版本。

若要返回模型查询的来源，请将 `include` 设置为 `["web_search_call.action.sources"]`。 匹配的源 URL 出现在 `action.sources` 输出项的 `web_search_call` 数组中。 每个条目都包含一个 `type` 和一个 `url`。 页面标题不会在 `action.sources` 中返回；相反，模型的依据文本会在消息项上的 `url_citation` 注释中包含这些标题。

若要返回所咨询模型的搜索结果片段，请设置为 `include``["web_search_call.results"]`。 仅当你使用推理模型时，才支持该 `web_search_call.results` 选项。

# [Python](#tab/python)
```python
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/"
token_provider = get_bearer_token_provider(
    DefaultAzureCredential(), "https://ai.azure.com/.default"
)

openai = OpenAI(base_url=endpoint, api_key=token_provider)

response = openai.responses.create(
    model="gpt-5.5",
    reasoning={"effort": "low"},
    tools=[
        {
            "type": "web_search",
            "filters": {
                "allowed_domains": [
                    "pubmed.ncbi.nlm.nih.gov",
                    "clinicaltrials.gov",
                    "www.who.int",
                    "www.cdc.gov",
                    "www.fda.gov",
                ],
                "blocked_domains": [
                    "en.wikipedia.org",
                    "www.reddit.com",
                ],
            },
        }
    ],
    tool_choice="auto",
    include=["web_search_call.action.sources"],
    input="Please perform a web search on how semaglutide is used in the treatment of diabetes.",
)

print(response.output_text)
```

# [C#](#tab/csharp)
```csharp
using Azure.Identity;
using OpenAI.Responses;
using System.ClientModel.Primitives;

#pragma warning disable OPENAI001

string endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

BearerTokenPolicy tokenPolicy = new(
    new DefaultAzureCredential(),
    "https://ai.azure.com/.default");

ResponsesClient openAIClient = new(
    authenticationPolicy: tokenPolicy,
    options: new ResponsesClientOptions { Endpoint = new Uri(endpoint) });

WebSearchToolFilters filters = new();
filters.AllowedDomains.Add("pubmed.ncbi.nlm.nih.gov");
filters.AllowedDomains.Add("clinicaltrials.gov");
filters.AllowedDomains.Add("www.who.int");
filters.AllowedDomains.Add("www.cdc.gov");
filters.AllowedDomains.Add("www.fda.gov");

// blocked_domains isn't yet a typed property on WebSearchToolFilters; use the
// JsonPatch escape hatch until the typed setter ships in OpenAI.Responses.
#pragma warning disable SCME0001
filters.Patch.Set("$.blocked_domains"u8,
    """["en.wikipedia.org","www.reddit.com"]"""u8);
#pragma warning restore SCME0001

CreateResponseOptions options = new()
{
    Model = "gpt-5.5",
    ReasoningOptions = new ResponseReasoningOptions { ReasoningEffortLevel = ResponseReasoningEffortLevel.Low },
    Tools = { ResponseTool.CreateWebSearchTool(filters: filters) },
    ToolChoice = ResponseToolChoice.CreateAutoChoice(),
    IncludedProperties = { IncludedResponseProperty.WebSearchCallActionSources }
};
options.InputItems.Add(ResponseItem.CreateUserMessageItem(
    "Please perform a web search on how semaglutide is used in the treatment of diabetes."));

ResponseResult response = await openAIClient.CreateResponseAsync(options);
Console.WriteLine(response.GetOutputText());
```

# [JavaScript](#tab/javascript)
```javascript
import { OpenAI } from "openai";
import { DefaultAzureCredential, getBearerTokenProvider } from "@azure/identity";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";
const tokenProvider = getBearerTokenProvider(
  new DefaultAzureCredential(),
  "https://ai.azure.com/.default"
);

const openai = new OpenAI({
  baseURL: endpoint,
  apiKey: await tokenProvider(),
});

const response = await openai.responses.create({
  model: "gpt-5.5",
  reasoning: { effort: "low" },
  tools: [
    {
      type: "web_search",
      filters: {
        allowed_domains: [
          "pubmed.ncbi.nlm.nih.gov",
          "clinicaltrials.gov",
          "www.who.int",
          "www.cdc.gov",
          "www.fda.gov",
        ],
        blocked_domains: [
          "en.wikipedia.org",
          "www.reddit.com",
        ],
      },
    },
  ],
  tool_choice: "auto",
  include: ["web_search_call.action.sources"],
  input: "Please perform a web search on how semaglutide is used in the treatment of diabetes.",
});

console.log(response.output_text);
```

# [Java](#tab/java)
```java
import com.azure.identity.AuthenticationUtil;
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.core.JsonValue;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;
import com.openai.models.responses.ResponseIncludable;
import com.openai.models.responses.ToolChoiceOptions;
import com.openai.models.responses.WebSearchTool;
import com.openai.models.responses.WebSearchTool.Filters;
import java.util.List;

public class WebSearchDomainFilterExample {
    public static void main(String[] args) {
        String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

        OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
            .baseUrl(endpoint)
            .credential(BearerTokenCredential.create(
                AuthenticationUtil.getBearerTokenSupplier(
                    new DefaultAzureCredentialBuilder().build(),
                    "https://ai.azure.com/.default")))
            .build();

        WebSearchTool webSearchTool = WebSearchTool.builder()
            .type(WebSearchTool.Type.WEB_SEARCH)
            .filters(Filters.builder()
                .addAllowedDomain("pubmed.ncbi.nlm.nih.gov")
                .addAllowedDomain("clinicaltrials.gov")
                .addAllowedDomain("www.who.int")
                .addAllowedDomain("www.cdc.gov")
                .addAllowedDomain("www.fda.gov")
                // blocked_domains isn't yet a typed builder on Filters; use
                // putAdditionalProperty until addBlockedDomain ships in openai-java.
                .putAdditionalProperty("blocked_domains", JsonValue.from(List.of(
                    "en.wikipedia.org",
                    "www.reddit.com")))
                .build())
            .build();

        ResponseCreateParams params = ResponseCreateParams.builder()
            .model("gpt-5.5")
            .input("Please perform a web search on how semaglutide is used in the treatment of diabetes.")
            .addTool(webSearchTool)
            .toolChoice(ToolChoiceOptions.AUTO)
            .addInclude(ResponseIncludable.WEB_SEARCH_CALL_ACTION_SOURCES)
            .build();

        Response response = openAIClient.responses().create(params);
        response.output().forEach(item -> item.message().ifPresent(msg ->
            msg.content().forEach(content -> content.outputText().ifPresent(
                t -> System.out.println(t.text())))));
    }
}
```

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AZURE_OPENAI_AUTH_TOKEN" \
  -d '{
    "model": "gpt-5.5",
    "reasoning": { "effort": "low" },
    "tools": [
      {
        "type": "web_search",
        "filters": {
          "allowed_domains": [
            "pubmed.ncbi.nlm.nih.gov",
            "clinicaltrials.gov",
            "www.who.int",
            "www.cdc.gov",
            "www.fda.gov"
          ],
          "blocked_domains": [
            "en.wikipedia.org",
            "www.reddit.com"
          ]
        }
      }
    ],
    "tool_choice": "auto",
    "include": ["web_search_call.action.sources"],
    "input": "Please perform a web search on how semaglutide is used in the treatment of diabetes."
  }'
```

---

若要使用 API 密钥身份验证，请将Microsoft Entra凭据替换为 API 密钥，如 basic 示例所示。

### 局限性

- 不支持实时 Internet 访问。 Azure OpenAI 始终将 `external_web_access` 参数视为 `false`。
- 域允许列表最多支持 100 个 URL。
- Web 搜索调用操作会产生工具调用成本。 有关详细信息，请参阅[定价](https://www.microsoft.com/en-us/bing/apis)。
- 支持网络搜索工具（`web_search_preview`）的预览版本，但不建议使用。
- `open_page` 和 `find_in_page` 操作仅适用于推理模型。

## 管理 Web 搜索工具

可以使用 Azure CLI 在订阅级别的响应 API 中启用或禁用 `web_search` 工具。 此设置适用于指定订阅中的所有帐户。

### 先决条件

在运行以下命令之前，请确保满足以下先决条件：

- [Azure CLI](/zh-cn/cli/azure/install-azure-cli)已安装。
- 你已使用 `az login` 登录到 Azure。
- 你对该订阅具有 **所有者** 或 **参与者** 访问权限。

### 禁用 Web 搜索

若要在订阅中禁用所有账户的`web_search`工具，您可以执行以下步骤：

```bash
az feature register --name OpenAI.BlockedTools.web_search --namespace Microsoft.CognitiveServices --subscription "<subscription-id>"
```

此命令禁用指定订阅中所有帐户的 Web 搜索。

### 启用 Web 搜索

若要启用该工具，请执行以下操作 `web_search` ：

```bash
az feature unregister --name OpenAI.BlockedTools.web_search --namespace Microsoft.CognitiveServices --subscription "<subscription-id>"
```

此命令为订阅所有帐户启用必应网络搜索功能。

## 故障 排除

- **未返回引文**：确认请求包括 `tools: [{"type": "web_search"}]`。 如果模型未调用该工具，请更显式地提示浏览 Web 或请求引文。
- **工具被阻止**：要求订阅管理员验证阻止工具的订阅功能设置。 请参阅 “管理 Web 搜索”工具。
- **身份验证错误**：对于 API 密钥，请验证是否已设置 `AZURE_OPENAI_API_KEY`。 对于Microsoft Entra ID，请验证令牌范围是否为 `https://ai.azure.com/.default`。