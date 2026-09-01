---
layout: Conceptual
title: 将 shell 工具与响应 API 配合使用 - Microsoft Foundry | Microsoft Learn
canonicalUrl: https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/shells
schema: Conceptual
ai-usage: ai-assisted
author: alvinashcraft
breadcrumb_path: ../../../breadcrumb/azure-ai/toc.json
depot_name: Learn.azure-ai
description: 了解如何使用 Azure OpenAI 响应 API（包括容器重用和可下载的项目）在托管和本地 shell 环境中运行命令。
feedback_help_link_type: get-help-at-qna
feedback_help_link_url: https://learn.microsoft.com/answers/tags/133/azure
feedback_product_url: https://feedback.azure.com/d365community/forum/79b1327d-d925-ec11-b6e6-000d3a4f06a4
feedback_system: Standard
git_commit_id: 71b24805b97486e7ff35ebd1af5068b9ce6b1d66
gitcommit: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/71b24805b97486e7ff35ebd1af5068b9ce6b1d66/articles/foundry/openai/how-to/shells.md
learn_banner_products:
- azure
locale: zh-cn
manager: mcleanbyron
ms.author: aashcraft
ms.collection: ce-skilling-ai-copilot
ms.custom:
- doc-kit-assisted
ms.date: 2026-06-04T00:00:00.0000000Z
ms.service: microsoft-foundry
ms.subservice: foundry-openai
ms.suite: office
ms.topic: how-to
ms.update-cycle: 90-days
original_content_git_url: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/live/articles/foundry/openai/how-to/shells.md
permissioned-type: public
recommendation_types:
- Training
- Certification
recommendations: true
site_name: Docs
uhfHeaderId: azure-ai-foundry
updated_at: 2026-08-21T00:46:00.0000000Z
ms.translationtype: HT
ms.contentlocale: zh-cn
loc_version: 2026-06-05T23:29:41.9374590Z
loc_source_id: Github-845157915#live
loc_file_id: Github-845157915.live.Learn.azure-ai.articles/foundry/openai/how-to/shells.md
page_type: conceptual
toc_rel: ../../toc.json
word_count: 2112
asset_id: foundry/openai/how-to/shells
item_type: Content
platformId: f2aa5a41-00a7-86aa-5b95-a55f0627c84d
---

# 将 shell 工具与响应 API 配合使用 - Microsoft Foundry | Microsoft Learn

shell 工具在完整的终端环境中作为 **响应 API** 调用的一部分运行命令。 使用它来运行脚本、处理文件和执行程序。 shell 工具支持两种执行模式：

- **Hosted shell**：Azure OpenAI 预配和管理请求的沙盒容器。
- **本地 shell**：在自己的运行时中执行模型的 `shell_call` 操作并返回结果。

只能通过响应 API 访问 shell 工具。 无法通过 Chat Completions API 获取。

Important

运行任意 shell 命令可能很危险。 始终在沙盒中执行，尽可能采用允许列表或阻止列表，并记录工具活动以供审计。

注释

shell 工具需要支持它的 Azure OpenAI API 版本，以及支持响应 API 的模型部署。 在部署到生产环境之前，请确认对目标 API 版本的支持。

## 先决条件

- 部署Azure OpenAI 模型，该模型支持响应 API 和 shell 工具。
- 身份验证方法：
    - API 密钥，或
    - Microsoft Entra ID。
- 安装适用于语言的客户端库：
    - **Python**： `pip install openai azure-identity`
    - **JavaScript/TypeScript**： `npm install openai @azure/identity`
    - **Java**：向项目添加 `com.openai:openai-java` 和 `com.azure:azure-identity`。
- 对于 REST 示例，请设置 `AZURE_OPENAI_API_KEY` （API 密钥流）或 `AZURE_OPENAI_AUTH_TOKEN` （Microsoft Entra ID 流）。

## 使用托管 shell 运行第一个命令

托管 Shell 是最快的入门方式。 将环境设置为 `container_auto`，以便Azure OpenAI 预配和管理请求的容器。 模型决定是否根据提示调用该工具。

在以下示例中，请将 `gpt-5.5` 替换为您自己的模型部署名称。

# [Python](#tab/python)
```python
from openai import OpenAI
from azure.identity import DefaultAzureCredential, get_bearer_token_provider

endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/"
token_provider = get_bearer_token_provider(
    DefaultAzureCredential(), "https://ai.azure.com/.default"
)

# Create the client against the Azure OpenAI v1 endpoint.
openai = OpenAI(base_url=endpoint, api_key=token_provider)

# Run a command in an auto-provisioned hosted container.
response = openai.responses.create(
    model="gpt-5.5",
    tools=[{"type": "shell", "environment": {"type": "container_auto"}}],
    input="Run: python --version && echo 'hello from the shell'",
)

print(response.output_text)
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

// Create the client against the Azure OpenAI v1 endpoint.
const openai = new OpenAI({ baseURL: endpoint, apiKey: await tokenProvider() });

// Run a command in an auto-provisioned hosted container.
const response = await openai.responses.create({
  model: "gpt-5.5",
  tools: [{ type: "shell", environment: { type: "container_auto" } }],
  input: "Run: python --version && echo 'hello from the shell'",
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
import com.openai.models.responses.ContainerAuto;
import com.openai.models.responses.FunctionShellTool;
import com.openai.models.responses.Response;
import com.openai.models.responses.ResponseCreateParams;

public class ShellExample {
    public static void main(String[] args) {
        String endpoint = "https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1";

        // Create the client against the Azure OpenAI v1 endpoint.
        OpenAIClient openAIClient = OpenAIOkHttpClient.builder()
            .baseUrl(endpoint)
            .credential(BearerTokenCredential.create(
                AuthenticationUtil.getBearerTokenSupplier(
                    new DefaultAzureCredentialBuilder().build(),
                    "https://ai.azure.com/.default")))
            .build();

        // Run a command in an auto-provisioned hosted container.
        FunctionShellTool shellTool = FunctionShellTool.builder()
            .environment(ContainerAuto.builder().build())
            .build();

        ResponseCreateParams params = ResponseCreateParams.builder()
            .model("gpt-5.5")
            .input("Run: python --version && echo 'hello from the shell'")
            .addTool(shellTool)
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
      { "type": "shell", "environment": { "type": "container_auto" } }
    ],
    "input": "Execute: ls -lah /mnt/data && python --version"
    }'
```

---

Tip

这些示例使用Microsoft Entra ID。 若要改用 API 密钥，请将 `api_key` 设置为密钥值（Python和 JavaScript），在生成客户端（Java）时传递 `AzureApiKeyCredential`，或发送 `api-key` 标头而不是 `Authorization: Bearer` （REST）。

参考：[使用 Azure OpenAI 响应 API](responses) | [OpenAI Python SDK](https://github.com/openai/openai-python) | [OpenAI Node SDK](https://github.com/openai/openai-node) | [OpenAI Java SDK](https://github.com/openai/openai-java)

## 托管运行时详细信息

托管容器为每个请求或容器会话提供托管的 Linux 环境：

- 运行时基于 `Debian 12`，并且可能会随时间推移而变化。
- 默认工作目录为 `/mnt/data`。 此目录始终存在，且是用户可下载工件的受支持路径。
- Python `3.11` 已预装。
- 托管 shell 环境不支持交互式 TTY 会话，并且命令无法在 `sudo` 下运行。
- 托管容器没有出站网络访问权限。

工作流需要服务时，可以在容器中运行服务。

## 跨请求重用容器

对于迭代工作流，请创建一次容器，并在后续的响应 API 调用中引用它。 容器在处于活动状态时，会在请求之间保留文件和状态。

首先，创建容器。

# [Python](#tab/python)
```python
# Create a reusable container.
container = openai.containers.create(
    name="analysis-container",
    expires_after={"anchor": "last_active_at", "minutes": 20},
)

print(container.id)
```

# [JavaScript](#tab/javascript)
```javascript
// Create a reusable container.
const container = await openai.containers.create({
  name: "analysis-container",
  expires_after: { anchor: "last_active_at", minutes: 20 },
});

console.log(container.id);
```

# [Java](#tab/java)
注释

通过 **REST** 选项卡中显示的 REST API 创建和引用容器。如前面的Java示例中所示生成客户端，然后使用 REST 调用返回的容器 ID 继续工作流。

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/containers \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AZURE_OPENAI_AUTH_TOKEN" \
  -d '{
    "name": "analysis-container",
    "expires_after": { "anchor": "last_active_at", "minutes": 20 }
    }'
```

---

然后在 shell 请求中按 ID 引用容器。

# [Python](#tab/python)
```python
# Reference the existing container in a shell request.
response = openai.responses.create(
    model="gpt-5.5",
    tools=[{
        "type": "shell",
        "environment": {
            "type": "container_reference",
            "container_id": container.id,
        },
    }],
    input="List files in the container and show disk usage.",
)

print(response.output_text)
```

# [JavaScript](#tab/javascript)
```javascript
// Reference the existing container in a shell request.
const response = await openai.responses.create({
  model: "gpt-5.5",
  tools: [{
    type: "shell",
    environment: {
      type: "container_reference",
      container_id: container.id,
    },
  }],
  input: "List files in the container and show disk usage.",
});

console.log(response.output_text);
```

# [Java](#tab/java)
注释

通过 **REST 选项卡中显示的** REST API 引用现有容器。在 shell 工具 `container_reference` 的环境中传递容器 ID。

# [REST](#tab/rest)
```bash
curl -X POST https://YOUR-RESOURCE-NAME.openai.azure.com/openai/v1/responses \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AZURE_OPENAI_AUTH_TOKEN" \
  -d '{
    "model": "gpt-5.5",
    "tools": [
      {
        "type": "shell",
        "environment": {
          "type": "container_reference",
          "container_id": "<container_id>"
        }
      }
    ],
    "input": "List files in the container and show disk usage."
    }'
```

---

要在多轮中继续使用同一容器进行工作，请传递与前一个响应相同的 `container_id` 和 `previous_response_id`。

## 在本地 shell 模式下运行命令

在本地 shell 模式下，可以在自己的运行时而不是托管容器中执行模型的命令。 需要完全控制执行环境、文件系统访问或现有内部工具时，请使用此模式。

将环境设置为 `local`. 模型返回 `shell_call` 描述要运行的命令的项。

```python
response = openai.responses.create(
    model="gpt-5.5",
    instructions="The local shell environment is on Linux.",
    input="Find the largest PDF file in the current directory.",
    tools=[{"type": "shell", "environment": {"type": "local"}}],
)

print(response.model_dump_json(indent=2))
```

收到 `shell_call` 项时，请运行请求的命令，捕获输出，并在下一个请求中返回结果 `shell_call_output` 。 以下执行器会捕获 `stdout`、`stderr` 以及退出结果，并处理超时情况。

```python
import subprocess
from dataclasses import dataclass

@dataclass
class CmdResult:
    stdout: str
    stderr: str
    exit_code: int | None
    timed_out: bool

def run_command(cmd: str, timeout: float = 60) -> CmdResult:
    process = subprocess.Popen(
        cmd, shell=True, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True,
    )
    try:
        out, err = process.communicate(timeout=timeout)
        return CmdResult(out, err, process.returncode, False)
    except subprocess.TimeoutExpired:
        process.kill()
        out, err = process.communicate()
        return CmdResult(out, err, process.returncode, True)
```

参考：[使用 Azure OpenAI 响应 API](responses)

## 响应中的 Shell 输出

托管 shell 和本地 shell 使用相同的输出项类型。 每个 Shell 运行实例都由一对项目表示：

- `shell_call`：模型请求的命令。
- `shell_call_output`：命令输出和退出结果。

该模型请求包含 `shell_call` 项的命令：

```json
{
  "type": "shell_call",
  "call_id": "call_<id>",
  "action": {
    "commands": ["ls -l"],
    "timeout_ms": 120000,
    "max_output_length": 4096
  },
  "status": "in_progress"
}
```

你返回一个带有 `shell_call_output` 项的结果：

```json
{
  "type": "shell_call_output",
  "call_id": "call_<id>",
  "max_output_length": 4096,
  "output": [
    {
      "stdout": "...",
      "stderr": "",
      "outcome": { "type": "exit", "exit_code": 0 }
    }
  ]
}
```

如果 `shell_call` 包含 `max_output_length`，请在 `shell_call_output` 上包含相同的值。 如果命令超过执行超时时间，则返回 `timeout` 结果，并包含已捕获到的所有部分输出。

## 下载工件

托管 shell 可以生成可下载的文件。 若要检索项目，请将其写入 `/mnt/data`，然后使用代码解释器使用的相同容器和文件 API 下载它们。

## 数据保留和容器生命周期

- 托管容器会话在空闲 20 分钟后结束。
- 托管容器在创建后一小时删除。 删除容器时，将删除存储在容器中的所有数据。

托管容器可能会在容器处于活动状态时将临时应用程序状态写入容器文件系统（由临时块存储提供支持）。 容器数据在容器过期或删除时被删除。

## 处理常见错误

- **超时**：如果命令执行超时，则返回 `timeout` 结果，并包含已捕获的任何部分输出。
- **截断输出**：当 `shell_call` 上存在 `max_output_length` 时，在 `shell_call_output` 上设置相同的值。
- **交互式命令**：Shell 工具执行是非交互式的。 不要依赖于提示输入的命令。
- **非零退出**：保留非零退出的输出，以便模型可以解释恢复步骤的原因。