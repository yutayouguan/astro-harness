---
layout: Conceptual
title: 将技能与响应 API 配合使用 - Microsoft Foundry | Microsoft Learn
canonicalUrl: https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/skills
schema: Conceptual
ai-usage: ai-assisted
author: alvinashcraft
breadcrumb_path: ../../../breadcrumb/azure-ai/toc.json
depot_name: Learn.azure-ai
description: 了解如何在托管和本地环境中为 Azure OpenAI 响应 API shell 工具创建、装载和版本可重用技能捆绑包。
feedback_help_link_type: get-help-at-qna
feedback_help_link_url: https://learn.microsoft.com/answers/tags/133/azure
feedback_product_url: https://feedback.azure.com/d365community/forum/79b1327d-d925-ec11-b6e6-000d3a4f06a4
feedback_system: Standard
git_commit_id: 71b24805b97486e7ff35ebd1af5068b9ce6b1d66
gitcommit: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/71b24805b97486e7ff35ebd1af5068b9ce6b1d66/articles/foundry/openai/how-to/skills.md
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
original_content_git_url: https://github.com/MicrosoftDocs/azure-ai-docs-pr/blob/live/articles/foundry/openai/how-to/skills.md
permissioned-type: public
recommendation_types:
- Training
- Certification
recommendations: true
site_name: Docs
uhfHeaderId: azure-ai-foundry
updated_at: 2026-08-09T19:20:00.0000000Z
ms.translationtype: HT
ms.contentlocale: zh-cn
loc_version: 2026-06-05T23:29:41.9374590Z
loc_source_id: Github-845157915#live
loc_file_id: Github-845157915.live.Learn.azure-ai.articles/foundry/openai/how-to/skills.md
page_type: conceptual
toc_rel: ../../toc.json
word_count: 1739
asset_id: foundry/openai/how-to/skills
item_type: Content
platformId: 67b01d95-974c-69ff-dda6-477c8bb2ab99
---

# 将技能与响应 API 配合使用 - Microsoft Foundry | Microsoft Learn

技能是带版本的文件捆绑包，可在 **Responses API** 中跨 shell 环境复用。 使用技能来编码流程和约定（从公司风格指南到多步骤工作流）并在运行 [shell 工具](shells)时将其提供给模型。

技能是文件包加上 `SKILL.md` 清单。 清单包含 YAML 前置内容和提示样式的说明。 模型使用前置信息来识别该技能，并在决定调用该技能时阅读说明。 技能与开放 [代理技能标准](https://agentskills.io/home)兼容。

Important

技能可能会影响规划、工具选择和命令执行。 将每个技能视为特权代码，并在使用之前仔细查看它。 有关详细信息，请参阅 风险和安全。

注释

技能需要使用支持 shell 工具和技能的 Azure OpenAI API 版本。 在部署到生产环境之前，请确认对目标 API 版本的支持。

## 先决条件

- 部署Azure OpenAI 模型，该模型支持响应 API 和 shell 工具。
- 身份验证方法：
    - API 密钥，或
    - Microsoft Entra ID。
- 安装适用于语言的客户端库：
    - **Python**： `pip install openai azure-identity`
    - **JavaScript/TypeScript**： `npm install openai @azure/identity`
- 对于 REST 示例，请设置 `AZURE_OPENAI_API_KEY` （API 密钥流）或 `AZURE_OPENAI_AUTH_TOKEN` （Microsoft Entra ID 流）。

## 技能中包含的内容

技能包有一个包含 `SKILL.md` 清单和任何支持文件的顶级文件夹：

```text
csv-insights/
├── SKILL.md
├── scripts/
│   └── summarize.py
└── templates/
    └── report.md
```

`SKILL.md` 文件在 YAML 前置元数据中声明该技能的 `name` 和 `description`，并提供在调用该技能时模型应遵循的说明。 前端验证遵循 [代理技能规范](https://agentskills.io/specification#name-field)。

## 创建技能

使用以下任一格式上传技能捆绑包：

- **目录上传（多部分）**：上传多个文件。 每个部分都包含相对于单个顶级文件夹的文件路径。
- **Zip 上传**：压缩单个顶级文件夹并上传 `.zip` 文件。

上传会返回一个 `skill_id`，供你在将该技能附加到 shell 环境时引用。

## 将技能与托管命令行环境结合使用

若要使技能在托管 shell 环境中可用，请通过环境的 `skills` 数组附加它们。 装载技能后，模型会根据提示决定是否调用它。

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

# Mount skills in an auto-provisioned hosted container.
response = openai.responses.create(
    model="gpt-5.5",
    tools=[{
        "type": "shell",
        "environment": {
            "type": "container_auto",
            "skills": [{"type": "skill_reference", "skill_id": "<skill_id>"}],
        },
    }],
    input="Use the csv-insights skill to summarize report.csv.",
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

// Mount skills in an auto-provisioned hosted container.
const response = await openai.responses.create({
  model: "gpt-5.5",
  tools: [{
    type: "shell",
    environment: {
      type: "container_auto",
      skills: [{ type: "skill_reference", skill_id: "<skill_id>" }],
    },
  }],
  input: "Use the csv-insights skill to summarize report.csv.",
});

console.log(response.output_text);
```

---

Tip

装载技能后，模型决定何时使用它。 有关更确定性的行为，请指示模型使用命名技能，例如 *，使用 `csv-insights` 技能汇总报表。*

参考：[将 shell 工具与 Responses API 配合使用](shells) | [OpenAI Python SDK](https://github.com/openai/openai-python) | [OpenAI Node SDK](https://github.com/openai/openai-node)

## 将技能与本地 shell 模式配合使用

技能也适用于本地 Shell 模式。 不要使用 `skill_reference`，而应在你控制的运行时环境中通过本地路径提供技能文件。

# [Python](#tab/python)
```python
# Mount a skill from a local path in local shell mode.
response = openai.responses.create(
    model="gpt-5.5",
    tools=[{
        "type": "shell",
        "environment": {
            "type": "local",
            "skills": [{
                "name": "csv-insights",
                "description": "Summarize CSV files and produce a markdown report.",
                "path": "<path-to-skill-folder>",
            }],
        },
    }],
    input="Use the csv-insights skill to summarize today's CSV reports.",
)

print(response.output_text)
```

# [JavaScript](#tab/javascript)
```javascript
// Mount a skill from a local path in local shell mode.
const response = await openai.responses.create({
  model: "gpt-5.5",
  tools: [{
    type: "shell",
    environment: {
      type: "local",
      skills: [{
        name: "csv-insights",
        description: "Summarize CSV files and produce a markdown report.",
        path: "<path-to-skill-folder>",
      }],
    },
  }],
  input: "Use the csv-insights skill to summarize today's CSV reports.",
});

console.log(response.output_text);
```

---

注释

本地 shell 模式不支持上传的 `skill_reference` 附件。 请改为从本地路径提供技能文件。

## 内联技能

如果不想创建上传技能，可以在环境的 `skills` 数组中内联一个经 base64 编码的 zip 包。 如果希望技能仅在单个容器的生命周期内生存，则内联技能非常有用。

```python
import base64

# Read and encode a local skill bundle.
with open("csv_insights.zip", "rb") as f:
    inline_zip = base64.b64encode(f.read()).decode("utf-8")

# Create a container with the inline skill mounted.
container = openai.containers.create(
    name="inline-skill-container",
    skills=[{
        "type": "inline",
        "name": "csv-insights",
        "description": "Summarize CSV files and produce a markdown report.",
        "source": {
            "type": "base64",
            "media_type": "application/zip",
            "data": inline_zip,
        },
    }],
)

print(container.id)
```

## 版本控制和管理

技能有版本。 每次上传都会创建一个新版本，并且通过 `version` 字段引用 `skill_reference`版本。 该 `version` 字段接受整数或 `"latest"`。

两个指针跟踪版本：

- 如果你未在 `skill_reference` 中指定版本，则会使用 `default_version`。
- `latest_version` 跟踪最新的上传。

删除遵循以下规则：

- 无法删除默认版本。 首先将另一个版本设置为默认版本。
- 删除最后一个剩余版本会删除技能本身。
- 删除技能会删除其所有版本。

## 限制和验证

| Limit | 价值 |
| --- | --- |
| `SKILL.md` 每个包中的文件数 | 恰好一个（文件名匹配不区分大小写） |
| 最大 ZIP 文件上传大小 | 50 MB |
| 每个技能版本的最大文件数 | 500 |
| 最大未压缩文件大小 | 25 MB |

技能前端验证遵循 [代理技能规范](https://agentskills.io/specification#name-field)。

## 风险和安全

在 Responses API 中使用某项技能之前，务必先仔细检查该技能。 技能引入了安全风险，例如提示注入驱动的数据外泄和未经授权的命令执行。 遵循以下做法：

- **将技能视为特权代码和说明。** 技能内容可能会影响规划、工具使用情况和命令执行。 将任何技能视为可能不受信任的输入，直到验证它。
- **不要向最终用户公开开放技能目录。** 开放对任意技能的选择会增加提示注入、策略绕过以及未经审核的自动化所导致的破坏性操作风险。
- **在开发者层面整合技能。** 将每个技能映射到特定的产品工作流，防止最终用户选择任意技能，并阻止显式审批和策略检查背后的高影响操作。
- **需要对敏感操作进行审批。** 对于可执行写入操作或高影响操作的工作流，在执行前需要显式批准。