---
layout: Conceptual
title: 使用 Azure OpenAI 生成嵌入内容 - Microsoft Foundry | Microsoft Learn
canonicalUrl: https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/embeddings
schema: Conceptual
ai-usage: ai-assisted
author: alvinashcraft
breadcrumb_path: ../../../breadcrumb/azure-ai/toc.json
depot_name: Learn.azure-ai
description: 了解如何使用适用于 Python、C#、JavaScript、Java 和 Go 或 REST API 的当前 OpenAI SDK 通过 Azure OpenAI 生成矢量嵌入。
feedback_help_link_type: get-help-at-qna
feedback_help_link_url: https://learn.microsoft.com/answers/tags/133/azure
feedback_product_url: https://feedback.azure.com/d365community/forum/79b1327d-d925-ec11-b6e6-000d3a4f06a4
feedback_system: Standard
git_commit_id: e79abb845258513ef11babcdecf121d715d92d67
gitcommit: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/e79abb845258513ef11babcdecf121d715d92d67/articles/foundry/openai/how-to/embeddings.md
learn_banner_products:
- azure
locale: zh-cn
manager: mcleans
ms.author: aashcraft
ms.collection: ce-skilling-ai-copilot
ms.custom:
- devx-track-python
- classic-and-new
- doc-kit-assisted
ms.date: 2026-07-22T00:00:00.0000000Z
ms.service: microsoft-foundry
ms.subservice: foundry-openai
ms.suite: office
ms.topic: how-to
ms.update-cycle: 90-days
original_content_git_url: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/live/articles/foundry/openai/how-to/embeddings.md
permissioned-type: public
recommendation_types:
- Training
- Certification
recommendations: false
site_name: Docs
uhfHeaderId: azure-ai-foundry
updated_at: 2026-08-13T08:13:00.0000000Z
ms.translationtype: MT
ms.contentlocale: zh-cn
loc_version: 2026-07-22T22:18:06.3063428Z
loc_source_id: Github-845157915#live
loc_file_id: Github-845157915.live.Learn.azure-ai.articles/foundry/openai/how-to/embeddings.md
page_type: conceptual
toc_rel: ../../toc.json
word_count: 1616
asset_id: foundry/openai/how-to/embeddings
item_type: Content
cmProducts:
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/8a6e4dad-7050-4ce7-83f9-eb4123577a54
- https://authoring-docs-microsoft.poolparty.biz/devrel/68ec7f3a-2bc6-459f-b959-19beb729907d
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/1433a524-c01f-4b87-beab-670c040dea4f
spProducts:
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/0a5fc323-00ce-4c20-9095-41948f54c83f
- https://authoring-docs-microsoft.poolparty.biz/devrel/90370425-aca4-4a39-9533-d52e5e002a5d
- https://microsoft-devrel.poolparty.biz/DevRelOfferingOntology/312f1f05-a431-4193-8a4d-e6245d5966de
platformId: 9edd4517-acd9-3f57-648e-e8800874655f
---

# 使用 Azure OpenAI 生成嵌入内容 - Microsoft Foundry | Microsoft Learn

嵌入是浮点数的向量，表示文本的语义含义。 类似的文本生成接近于一起的矢量，这使得嵌入对矢量搜索、建议、分类和聚类分析很有用。

## 先决条件

- 一份 Azure 订阅。 如果您还没有，请[免费创建一个](https://azure.microsoft.com/free/)。
- 具有嵌入模型部署的 Azure OpenAI 资源。
- 资源终结点，例如 `https://YOUR-RESOURCE-NAME.openai.azure.com`。
- 对于 Microsoft Entra ID 身份验证，需要一个分配有 Azure OpenAI 资源的 `Cognitive Services OpenAI User` 角色的标识。 有关详细信息，请参阅 [Azure OpenAI 的基于角色的访问控制](/zh-cn/azure/ai-foundry/openai/how-to/role-based-access-control)。
- 用于本地身份验证的 [Azure CLI](/zh-cn/cli/azure/install-azure-cli)。
- 所选语言的运行时和包管理器。

有关特定于语言的设置指南，请参阅 [Azure OpenAI 支持的编程语言](../supported-languages)。

`model`每个请求中的值是Azure模型部署名称。 这些示例使用 `text-embedding-3-small`;如果部署具有其他名称，请替换它。

## 生成嵌入

将文本发送到嵌入终结点，并从响应中的第一项读取向量。

v1 嵌入 API 支持Microsoft Entra ID和 API 密钥身份验证。 建议使用Microsoft Entra ID，因为它可以避免存储长期凭据。 本文中的示例使用Microsoft Entra ID。

对于本地开发，请在运行 SDK 示例之前登录到 Azure：

```bash
az login
```

`DefaultAzureCredential`在本地使用已登录标识，并在应用程序在Azure中运行时使用托管标识。

还支持 API 密钥身份验证。 有关基于密钥的客户端配置，请参阅 [Azure OpenAI v1 API 指南](../api-version-lifecycle)。

# [Python](#tab/python-new)
安装 OpenAI 和Azure标识包：

```bash
pip install openai azure-identity
```

生成嵌入向量并打印其维度：

```python
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/"
token_provider = get_bearer_token_provider(DefaultAzureCredential(), "https://ai.azure.com/.default"
)
openai = OpenAI(base_url=endpoint,api_key=token_provider,
)

# Generate one embedding vector.
response = openai.embeddings.create(model="text-embedding-3-small",input="The quick brown fox jumped over the lazy dog.",
)
print(f"Embedding dimensions: {len(response.data[0].embedding)}")
```

```output
Embedding dimensions: <number>
```

参考：[`embeddings.create`](https://github.com/openai/openai-python/blob/main/src/openai/resources/embeddings.py)

# [C#](#tab/csharp)
安装 OpenAI 和Azure标识包：

```dotnetcli
dotnet add package OpenAI
dotnet add package Azure.Identity
```

生成嵌入向量并打印其维度：

```csharp
using Azure.Identity;
using OpenAI;
using OpenAI.Embeddings;
using System.ClientModel.Primitives;

#pragma warning disable OPENAI001

var endpoint = new Uri("https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/");
BearerTokenPolicy tokenPolicy = new(new DefaultAzureCredential(),"https://ai.azure.com/.default");
var openAIClient = new EmbeddingClient(model: "text-embedding-3-small",authenticationPolicy: tokenPolicy,options: new OpenAIClientOptions { Endpoint = endpoint });

// Generate one embedding vector.
OpenAIEmbedding embedding = openAIClient.GenerateEmbedding("The quick brown fox jumped over the lazy dog.");
Console.WriteLine($"Embedding dimensions: {embedding.ToFloats().Length}");
```

```output
Embedding dimensions: <number>
```

参考：[`EmbeddingClient.GenerateEmbedding`](https://github.com/openai/openai-dotnet/blob/main/OpenAI/src/Custom/Embeddings/EmbeddingClient.cs)

# [Javascript](#tab/javascript)
安装 OpenAI 和Azure标识包：

```bash
npm install openai @azure/identity
```

生成嵌入向量并打印其维度：

```javascript
import {DefaultAzureCredential,getBearerTokenProvider,
} from "@azure/identity";
import OpenAI from "openai";

const endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";
const tokenProvider = getBearerTokenProvider(new DefaultAzureCredential(),"https://ai.azure.com/.default",
);
const openai = new OpenAI({ baseURL: endpoint, apiKey: tokenProvider });

// Generate one embedding vector.
const response = await openai.embeddings.create({
  model: "text-embedding-3-small",
  input: "The quick brown fox jumped over the lazy dog.",
});
console.log(`Embedding dimensions: ${response.data[0].embedding.length}`);
```

```output
Embedding dimensions: <number>
```

参考：[`embeddings.create`](https://github.com/openai/openai-node/blob/main/src/resources/embeddings.ts)

# [Java](#tab/java)
对于 Maven，添加 OpenAI 和Azure标识包：

```xml
<dependencies><dependency>	<groupId>com.openai</groupId>	<artifactId>openai-java</artifactId>	<version>4.43.0</version></dependency><dependency>	<groupId>com.azure</groupId>	<artifactId>azure-identity</artifactId>	<version>1.18.4</version></dependency>
</dependencies>
```

有关 Gradle 设置，请参阅 [Azure OpenAI Java支持](../supported-languages?pivots=programming-language-java)。

生成嵌入向量并打印其维度：

```java
import com.azure.identity.AuthenticationUtil;
import com.azure.identity.DefaultAzureCredentialBuilder;
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.credential.BearerTokenCredential;
import com.openai.models.embeddings.EmbeddingCreateParams;

public class EmbeddingsExample {public static void main(String[] args) {	String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/";	OpenAIClient openAIClient = OpenAIOkHttpClient.builder()			.baseUrl(endpoint)			.credential(BearerTokenCredential.create(					AuthenticationUtil.getBearerTokenSupplier(							new DefaultAzureCredentialBuilder().build(),							"https://ai.azure.com/.default")))			.build();
	// Generate one embedding vector.	EmbeddingCreateParams params = EmbeddingCreateParams.builder()			.model("text-embedding-3-small")			.input("The quick brown fox jumped over the lazy dog.")			.encodingFormat(EmbeddingCreateParams.EncodingFormat.FLOAT)			.build();	int dimensions = openAIClient.embeddings().create(params)			.data().get(0).embedding().size();	System.out.println("Embedding dimensions: " + dimensions);}
}
```

```output
Embedding dimensions: <number>
```

参考：[`EmbeddingCreateParams`](https://github.com/openai/openai-java/blob/main/openai-java-example/src/main/java/com/openai/example/EmbeddingsExample.java)

# [Go](#tab/go)
安装 OpenAI Go 模块版本 3 和Azure标识模块：

```bash
go get github.com/openai/openai-go/v3
go get github.com/Azure/azure-sdk-for-go/sdk/azidentity
```

生成嵌入向量并打印其维度：

```go
package main

import ("context""fmt"
"github.com/Azure/azure-sdk-for-go/sdk/azidentity""github.com/openai/openai-go/v3""github.com/openai/openai-go/v3/azure""github.com/openai/openai-go/v3/option"
)

func main() {credential, err := azidentity.NewDefaultAzureCredential(nil)if err != nil { panic(err) }endpoint := "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/"openaiClient := openai.NewClient(	option.WithBaseURL(endpoint),	azure.WithTokenCredential(credential, azure.WithTokenCredentialScopes(		[]string{"https://ai.azure.com/.default"})))
// Generate one embedding vector.response, err := openaiClient.Embeddings.New(context.Background(), openai.EmbeddingNewParams{	Model: "text-embedding-3-small",	Input: openai.EmbeddingNewParamsInputUnion{OfString: openai.String(		"The quick brown fox jumped over the lazy dog.")},})if err != nil { panic(err) }fmt.Printf("Embedding dimensions: %d\n", len(response.Data[0].Embedding))
}
```

```output
Embedding dimensions: <number>
```

参考：[`Embeddings.New`](https://github.com/openai/openai-go/blob/main/embedding.go)

# [PowerShell](#tab/PowerShell)
将资源终结点存储在`AZURE_OPENAI_ENDPOINT`中。 然后获取访问令牌并生成嵌入：

```powershell
$Env:AZURE_OPENAI_ENDPOINT = "https://YOUR-RESOURCE-NAME.openai.azure.com"
```

```powershell
$endpoint = "$($env:AZURE_OPENAI_ENDPOINT.TrimEnd('/'))/openai/v1/embeddings"
$token = az account get-access-token `--resource https://cognitiveservices.azure.com `--query accessToken `--output tsv
$headers = @{ Authorization = "Bearer $token" }

# Generate one embedding vector.
$body = @{
    model = "text-embedding-3-small"
    input = "The quick brown fox jumped over the lazy dog."
} | ConvertTo-Json
$response = Invoke-RestMethod `
    -Uri $endpoint `
    -Method Post `
    -Headers $headers `
    -ContentType "application/json" `
    -Body $body
Write-Output "Embedding dimensions: $($response.data[0].embedding.Count)"
```

```output
Embedding dimensions: <number>
```

参考： [`Invoke-RestMethod`](/zh-cn/powershell/module/microsoft.powershell.utility/invoke-restmethod) 和 [`az account get-access-token`](/zh-cn/cli/azure/account#az-account-get-access-token)

# [休息](#tab/console)
获取访问令牌，并将请求发送到 v1 嵌入终结点：

```bash
AZURE_OPENAI_AUTH_TOKEN=$(az account get-access-token \--resource https://cognitiveservices.azure.com \--query accessToken \--output tsv)
curl "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/embeddings" \
  -H "Content-Type: application/json" \-H "Authorization: Bearer $AZURE_OPENAI_AUTH_TOKEN" \
  -d '{"model":"text-embedding-3-small","input":"The quick brown fox."}'
```

```output
{"data":[{"embedding":[<vector-values>]}]}
```

参考：[Azure OpenAI 嵌入 REST API](/zh-cn/rest/api/microsoft-foundry/azureopenai/embeddings) 和 [`az account get-access-token`](/zh-cn/cli/azure/account#az-account-get-access-token)

---

## 最佳做法

Tip

当输入令牌 **的总和** 超过 300,000 时，嵌入请求将返回 HTTP 400，即使每个输入都低于每个输入限制。 将大型批处理拆分为较小的请求。

### 验证输入是否超过允许的最大长度

- 当前嵌入模型的最大输入长度为 8,192 个标记。 在发送请求之前检查每个输入。
- 如果在单个嵌入请求中发送输入数组，则最大数组大小为 2,048。
- 每个 `/embeddings` 请求的所有输入的总计上限为 300,000 个令牌。 超出此限制的请求将返回 HTTP 400 错误。
- 将每分钟令牌总数保持在分配给模型部署的配额以下。 有关当前限制，请参阅 [Azure OpenAI 配额和限制](../quotas-limits)。

## 故障 排除

- 对于 `401` 响应，请重新登录并确认访问令牌使用了正确的受众。
- 对于 `403` 响应，请确认你的标识分配有 Azure OpenAI 资源的 `Cognitive Services OpenAI User` 角色。
- 对于 `404` 响应，请确认终结点包含 `/openai/v1/`，并且 `model` 包含有效的部署名称。
- 对于 `400` 响应，请检查请求正文、每个输入的令牌数、输入数量以及总令牌数。

## 限制和风险

在某些情况下，嵌入模型可能不可靠或构成社会风险。 如果在不使用缓解措施的情况下使用，它们可能会造成损害。 有关如何负责任地使用它们的详细信息，请参阅[“负责任的 AI”](/zh-cn/azure/foundry/responsible-use-of-ai-overview)内容。