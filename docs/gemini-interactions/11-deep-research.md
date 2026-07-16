# 1 Deep Research 智能体

> 来源：[Google AI for Developers](https://ai.google.dev/gemini-api/docs/deep-research?hl=zh-cn)  
> 抓取说明：官方文档 **Interactions API** 版本（页面默认版本）。  
> 原文标题：Gemini Deep Research Agent

---

Gemini Deep Research 智能体可自主规划、执行和汇总多步骤研究任务。它由 Gemini 提供支持，可浏览复杂的信息环境，生成详细且引证详实的报告。借助新功能，您可以与智能体协同规划，使用 MCP 服务器连接到外部工具，添加可视化内容（如图表和图形），并直接提供文档作为输入。

研究任务涉及迭代搜索和阅读，可能需要几分钟才能完成。您必须使用[后台执行](https://ai.google.dev/gemini-api/docs/background-execution?hl=zh-cn)（设置`background=true`）
来异步运行智能体并轮询结果或流式传输更新。如需了解详情，请参阅
[处理长时间运行的任务](#long-running-tasks)。

`generate_content`
以下示例展示了如何在后台启动研究任务并轮询结果。

### 1.1.1 Python

```python
import time
from google import genai

client = genai.Client()

interaction = client.interactions.create(
    input="Research the history of Google TPUs.",
    agent="deep-research-preview-04-2026",
    background=True,
)

print(f"Research started: {interaction.id}")

while True:
    interaction = client.interactions.get(interaction.id)
    if interaction.status == "completed":
        print(interaction.steps[-1].content[0].text)
        break
    elif interaction.status == "failed":
        print(f"Research failed: {interaction.error}")
        break
    time.sleep(10)
```

### 1.1.2 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const interaction = await client.interactions.create({
    input: 'Research the history of Google TPUs.',
    agent: 'deep-research-preview-04-2026',
    background: true
});

console.log(`Research started: ${interaction.id}`);

while (true) {
    const result = await client.interactions.get(interaction.id);
    if (result.status === 'completed') {
        console.log(result.steps.at(-1).content[0].text);
        break;
    } else if (result.status === 'failed') {
        console.log(`Research failed: ${result.error}`);
        break;
    }
    await new Promise(resolve => setTimeout(resolve, 10000));
}
```

### 1.1.3 REST

```
# 1. Start the research task
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "input": "Research the history of Google TPUs.",
    "agent": "deep-research-preview-04-2026",
    "background": true
}'

# 2. Poll for results (Replace INTERACTION_ID)
# curl -X GET "https://generativelanguage.googleapis.com/v1beta/interactions/INTERACTION_ID" \
# -H "x-goog-api-key: $GEMINI_API_KEY"
```

## 1.2 支持的版本

Deep Research 智能体有两个版本：

- **Deep Research** (`deep-research-preview-04-2026`)：专为速度和效率而设计，非常适合流式传输回客户端界面。
- **Deep Research Max** (`deep-research-max-preview-04-2026`)：可自动收集和汇总上下文，实现最大程度的全面性。

## 1.3 协同规划

借助协同规划，您可以在智能体开始工作之前控制研究方向，方法是在执行之前查看和完善研究计划。启用后，智能体会返回建议的研究计划，而不是立即执行。然后，您可以通过多轮互动查看、修改或批准该计划。

### 1.3.1 第 1 步：请求计划

在第一次互动中设置 `collaborative_planning=True`。智能体会返回研究计划，而不是完整报告。

### 1.3.2 Python

```python
from google import genai

client = genai.Client()

# First interaction: request a research plan
plan_interaction = client.interactions.create(
    agent="deep-research-preview-04-2026",
    input="Do some research on Google TPUs.",
    agent_config={
        "type": "deep-research",
        "thinking_summaries": "auto",
        "collaborative_planning": True,
    },
    background=True,
)

# Wait for and retrieve the plan
while (result := client.interactions.get(id=plan_interaction.id)).status != "completed":
    time.sleep(5)
print(result.steps[-1].content[0].text)
```

### 1.3.3 JavaScript

```javascript
const planInteraction = await client.interactions.create({
    agent: 'deep-research-preview-04-2026',
    input: 'Do some research on Google TPUs.',
    agent_config: {
        type: 'deep-research',
        thinking_summaries: 'auto',
        collaborative_planning: true
    },
    background: true
});

let result;
while ((result = await client.interactions.get(planInteraction.id)).status !== 'completed') {
    await new Promise(r => setTimeout(r, 5000));
}
console.log(result.steps.at(-1).content[0].text);
```

### 1.3.4 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "agent": "deep-research-preview-04-2026",
    "input": "Do some research on Google TPUs.",
    "agent_config": {
        "type": "deep-research",
        "thinking_summaries": "auto",
        "collaborative_planning": true
    },
    "background": true
}'
```

### 1.3.5 第 2 步：完善计划（可选）

使用 `previous_interaction_id` 继续对话并迭代计划。保持 `collaborative_planning=True` 以保持规划模式。

### 1.3.6 Python

```
# Second interaction: refine the plan
refined_plan = client.interactions.create(
    agent="deep-research-preview-04-2026",
    input="Focus more on the differences between Google TPUs and competitor hardware, and less on the history.",
    agent_config={
        "type": "deep-research",
        "thinking_summaries": "auto",
        "collaborative_planning": True,
    },
    previous_interaction_id=plan_interaction.id,
    background=True,
)

while (result := client.interactions.get(id=refined_plan.id)).status != "completed":
    time.sleep(5)
print(result.steps[-1].content[0].text)
```

### 1.3.7 JavaScript

```javascript
const refinedPlan = await client.interactions.create({
    agent: 'deep-research-preview-04-2026',
    input: 'Focus more on the differences between Google TPUs and competitor hardware, and less on the history.',
    agent_config: {
        type: 'deep-research',
        thinking_summaries: 'auto',
        collaborative_planning: true
    },
    previous_interaction_id: planInteraction.id,
    background: true
});

let result;
while ((result = await client.interactions.get(refinedPlan.id)).status !== 'completed') {
    await new Promise(r => setTimeout(r, 5000));
}
console.log(result.steps.at(-1).content[0].text);
```

### 1.3.8 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "agent": "deep-research-preview-04-2026",
    "input": "Focus more on the differences between Google TPUs and competitor hardware, and less on the history.",
    "agent_config": {
        "type": "deep-research",
        "thinking_summaries": "auto",
        "collaborative_planning": true
    },
    "previous_interaction_id": "PREVIOUS_INTERACTION_ID",
    "background": true
}'
```

### 1.3.9 第 3 步：批准并执行

设置 `collaborative_planning=False`（或省略）以批准计划并开始研究。

### 1.3.10 Python

```
# Third interaction: approve the plan and kick off research
final_report = client.interactions.create(
    agent="deep-research-preview-04-2026",
    input="Plan looks good!",
    agent_config={
        "type": "deep-research",
        "thinking_summaries": "auto",
        "collaborative_planning": False,
    },
    previous_interaction_id=refined_plan.id,
    background=True,
)

while (result := client.interactions.get(id=final_report.id)).status != "completed":
    time.sleep(5)
print(result.steps[-1].content[0].text)
```

### 1.3.11 JavaScript

```javascript
const finalReport = await client.interactions.create({
    agent: 'deep-research-preview-04-2026',
    input: 'Plan looks good!',
    agent_config: {
        type: 'deep-research',
        thinking_summaries: 'auto',
        collaborative_planning: false
    },
    previous_interaction_id: refinedPlan.id,
    background: true
});

let result;
while ((result = await client.interactions.get(finalReport.id)).status !== 'completed') {
    await new Promise(r => setTimeout(r, 5000));
}
console.log(result.steps.at(-1).content[0].text);
```

### 1.3.12 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "agent": "deep-research-preview-04-2026",
    "input": "Plan looks good!",
    "agent_config": {
        "type": "deep-research",
        "thinking_summaries": "auto",
        "collaborative_planning": false
    },
    "previous_interaction_id": "PREVIOUS_INTERACTION_ID",
    "background": true
}'
```

## 1.4 可视化

当 `visualization` 设置为 `"auto"` 时，智能体可以生成图表、
图形和其他视觉元素来支持其研究结果。
生成的图片包含在响应步骤中，并以 `image` 增量流式传输。为获得最佳结果，请在查询中明确要求提供视觉内容，例如“包含显示随时间变化的趋势的图表”或“生成比较市场份额的图形”。 将 `visualization` 设置为
`"auto"` 可启用该功能，但智能体仅
在提示要求提供视觉内容时才会生成视觉内容。

### 1.4.1 Python

```python
import base64
import time

from google import genai

client = genai.Client()

interaction = client.interactions.create(
    agent="deep-research-preview-04-2026",
    input="Analyze global semiconductor market trends. Include graphics showing market share changes.",
    agent_config={
        "type": "deep-research",
        "visualization": "auto",
    },
    background=True,
)

print(f"Research started: {interaction.id}")

while (result := client.interactions.get(id=interaction.id)).status != "completed":
    time.sleep(5)

for step in result.steps:
    if step.type == "model_output":
        for content_item in step.content:
            if content_item.type == "text":
                print(content_item.text)
            elif content_item.type == "image" and content_item.data:
                image_bytes = base64.b64decode(content_item.data)
                print(f"Received image: {len(image_bytes)} bytes")
```

### 1.4.2 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const interaction = await client.interactions.create({
    agent: 'deep-research-preview-04-2026',
    input: 'Analyze global semiconductor market trends. Include graphics showing market share changes.',
    agent_config: {
        type: 'deep-research',
        visualization: 'auto'
    },
    background: true
});

console.log(`Research started: ${interaction.id}`);

let result;
while ((result = await client.interactions.get(interaction.id)).status !== 'completed') {
    await new Promise(r => setTimeout(r, 5000));
}

for (const step of result.steps) {
    if (step.type === 'model_output') {
        for (const contentItem of step.content) {
            if (contentItem.type === 'text') {
                console.log(contentItem.text);
            } else if (contentItem.type === 'image' && contentItem.data) {
                console.log(`[Image Output: ${contentItem.data.substring(0, 20)}...]`);
            }
        }
    }
}
```

### 1.4.3 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "agent": "deep-research-preview-04-2026",
    "input": "Analyze global semiconductor market trends. Include graphics showing market share changes.",
    "agent_config": {
        "type": "deep-research",
        "visualization": "auto"
    },
    "background": true
}'
```

## 1.5 支持的工具

Deep Research 支持多种内置工具和外部工具。默认情况下（未提供 `tools` 参数时），智能体可以访问 Google 搜索、网址上下文和代码执行。您可以明确指定工具来限制或扩展智能体的功能。

| 工具 | 类型值 | 说明 |
| --- | --- | --- |
| Google 搜索 | `google_search` | 搜索公开网络。默认处于启用状态。 |
| 网址上下文 | `url_context` | 读取和总结网页内容。默认处于启用状态。 |
| 代码执行 | `code_execution` | 执行代码以进行计算和数据分析。默认处于启用状态。 |
| MCP 服务器 | `mcp_server` | 连接到远程 MCP 服务器以访问外部工具。 |
| 文件搜索 | `file_search` | 搜索您上传的文档语料库。 |

### 1.5.1 Google 搜索

明确启用 Google 搜索作为唯一工具：

### 1.5.2 Python

```
interaction = client.interactions.create(
    agent="deep-research-preview-04-2026",
    input="What are the latest developments in quantum computing?",
    tools=[{"type": "google_search"}],
    background=True,
)
```

### 1.5.3 JavaScript

```
const interaction = await client.interactions.create({
    agent: 'deep-research-preview-04-2026',
    input: 'What are the latest developments in quantum computing?',
    tools: [{ type: 'google_search' }],
    background: true
});
```

### 1.5.4 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "agent": "deep-research-preview-04-2026",
    "input": "What are the latest developments in quantum computing?",
    "tools": [{"type": "google_search"}],
    "background": true
}'
```

### 1.5.5 网址上下文

让智能体能够读取和总结特定网页：

### 1.5.6 Python

```
interaction = client.interactions.create(
    agent="deep-research-preview-04-2026",
    input="Summarize the content of https://www.wikipedia.org/.",
    tools=[{"type": "url_context"}],
    background=True,
)
```

### 1.5.7 JavaScript

```
const interaction = await client.interactions.create({
    agent: 'deep-research-preview-04-2026',
    input: 'Summarize the content of https://www.wikipedia.org/.',
    tools: [{ type: 'url_context' }],
    background: true
});
```

### 1.5.8 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "agent": "deep-research-preview-04-2026",
    "input": "Summarize the content of https://www.wikipedia.org/.",
    "tools": [{"type": "url_context"}],
    "background": true
}'
```

### 1.5.9 代码执行

允许智能体执行代码以进行计算和数据分析：

### 1.5.10 Python

```
interaction = client.interactions.create(
    agent="deep-research-preview-04-2026",
    input="Calculate the 50th Fibonacci number.",
    tools=[{"type": "code_execution"}],
    background=True,
)
```

### 1.5.11 JavaScript

```
const interaction = await client.interactions.create({
    agent: 'deep-research-preview-04-2026',
    input: 'Calculate the 50th Fibonacci number.',
    tools: [{ type: 'code_execution' }],
    background: true
});
```

### 1.5.12 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "input": "Calculate the 50th Fibonacci number.",
    "agent": "deep-research-preview-04-2026",
    "tools": [{"type": "code_execution"}],
    "background": true
}'
```

### 1.5.13 MCP 服务器

连接到远程 MCP 服务器，让智能体能够访问外部工具和服务。

在工具配置中提供服务器 `name` 和 `url`。您还可以传递身份验证凭据，并限制智能体可以调用的工具。

| 字段 | 类型 | 是否必需 | 说明 |
| --- | --- | --- | --- |
| `type` | `string` | 是 | 必须为 `"mcp_server"`。 |
| `name` | `string` | 否 | MCP 服务器的显示名称。 |
| `url` | `string` | 否 | MCP 服务器端点的完整网址。 |
| `headers` | `object` | 否 | 作为 HTTP 标头随每个请求发送到服务器的键值对（例如身份验证令牌）。 |
| `allowed_tools` | `array` | 否 | 限制智能体可以从服务器调用的工具。 |

#### 1.5.13.1 基本用法

### 1.5.14 Python

```
interaction = client.interactions.create(
    agent="deep-research-preview-04-2026",
    input="Check the status of my last server deployment.",
    tools=[
        {
            "type": "mcp_server",
            "name": "Deployment Tracker",
            "url": "https://mcp.example.com/mcp",
            "headers": {"Authorization": "Bearer my-token"},
        }
    ],
    background=True,
)
```

### 1.5.15 JavaScript

```
const interaction = await client.interactions.create({
    agent: 'deep-research-preview-04-2026',
    input: 'Check the status of my last server deployment.',
    tools: [
        {
            type: 'mcp_server',
            name: 'Deployment Tracker',
            url: 'https://mcp.example.com/mcp',
            headers: { Authorization: 'Bearer my-token' }
        }
    ],
    background: true
});
```

### 1.5.16 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "agent": "deep-research-preview-04-2026",
    "input": "Check the status of my last server deployment.",
    "tools": [
        {
            "type": "mcp_server",
            "name": "Deployment Tracker",
            "url": "https://mcp.example.com/mcp",
            "headers": {"Authorization": "Bearer my-token"}
        }
    ],
    "background": true
}'
```

### 1.5.17 文件搜索

使用[文件搜索](https://ai.google.dev/gemini-api/docs/file-search?hl=zh-cn)工具让智能体能够访问您自己的数据。

### 1.5.18 Python

```python
import time
from google import genai

client = genai.Client()

interaction = client.interactions.create(
    input="Compare our 2025 fiscal year report against current public web news.",
    agent="deep-research-preview-04-2026",
    background=True,
    tools=[
        {
            "type": "file_search",
            "file_search_store_names": ['fileSearchStores/my-store-name']
        }
    ]
)
```

### 1.5.19 JavaScript

```
const interaction = await client.interactions.create({
    input: 'Compare our 2025 fiscal year report against current public web news.',
    agent: 'deep-research-preview-04-2026',
    background: true,
    tools: [
        { type: 'file_search', file_search_store_names: ['fileSearchStores/my-store-name'] },
    ]
});
```

### 1.5.20 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "input": "Compare our 2025 fiscal year report against current public web news.",
    "agent": "deep-research-preview-04-2026",
    "background": true,
    "tools": [
        {"type": "file_search", "file_search_store_names": ["fileSearchStores/my-store-name"]},
    ]
}'
```

## 1.6 可控性和格式设置

您可以在提示中提供具体的格式设置说明，以控制智能体的输出。这样，您就可以将报告划分为特定的部分和
子部分，添加数据表，或针对不同的受众群体调整语气（例如
“技术”“高管”“随意”）。

在输入文本中明确定义所需的输出格式。

### 1.6.1 Python

```
prompt = """
Research the competitive landscape of EV batteries.

Format the output as a technical report with the following structure:
1. Executive Summary
2. Key Players (Must include a data table comparing capacity and chemistry)
3. Supply Chain Risks
"""

interaction = client.interactions.create(
    input=prompt,
    agent="deep-research-preview-04-2026",
    background=True
)
```

### 1.6.2 JavaScript

```
const prompt = `
Research the competitive landscape of EV batteries.

Format the output as a technical report with the following structure:
1. Executive Summary
2. Key Players (Must include a data table comparing capacity and chemistry)
3. Supply Chain Risks
`;

const interaction = await client.interactions.create({
    input: prompt,
    agent: 'deep-research-preview-04-2026',
    background: true,
});
```

### 1.6.3 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "input": "Research the competitive landscape of EV batteries.\n\nFormat the output as a technical report with the following structure: \n1. Executive Summary\n2. Key Players (Must include a data table comparing capacity and chemistry)\n3. Supply Chain Risks",
    "agent": "deep-research-preview-04-2026",
    "background": true
}'
```

## 1.7 多模态输入

Deep Research 支持多模态输入，包括图片和文档 (PDF)，让智能体能够分析视觉内容，并根据提供的输入进行基于网络的上下文研究。

### 1.7.1 Python

```python
import time
from google import genai

client = genai.Client()

prompt = """Analyze the interspecies dynamics and behavioral risks present
in the provided image of the African watering hole. Specifically, investigate
the symbiotic relationship between the avian species and the pachyderms
shown, and conduct a risk assessment for the reticulated giraffes based on
their drinking posture relative to the specific predator visible in the
foreground."""

interaction = client.interactions.create(
    input=[
        {"type": "text", "text": prompt},
        {
            "type": "image",
            "mime_type": "image/jpeg",
            "uri": "https://storage.googleapis.com/generativeai-downloads/images/generated_elephants_giraffes_zebras_sunset.jpg"
        }
    ],
    agent="deep-research-preview-04-2026",
    background=True
)

print(f"Research started: {interaction.id}")

while True:
    interaction = client.interactions.get(interaction.id)
    if interaction.status == "completed":
        print(interaction.steps[-1].content[0].text)
        break
    elif interaction.status == "failed":
        print(f"Research failed: {interaction.error}")
        break
    time.sleep(10)
```

### 1.7.2 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const prompt = `Analyze the interspecies dynamics and behavioral risks present
in the provided image of the African watering hole. Specifically, investigate
the symbiotic relationship between the avian species and the pachyderms
shown, and conduct a risk assessment for the reticulated giraffes based on
their drinking posture relative to the specific predator visible in the
foreground.`;

const interaction = await client.interactions.create({
    input: [
        { type: 'text', text: prompt },
        {
            type: 'image',
            mime_type: "image/jpeg",
            uri: 'https://storage.googleapis.com/generativeai-downloads/images/generated_elephants_giraffes_zebras_sunset.jpg'
        }
    ],
    agent: 'deep-research-preview-04-2026',
    background: true
});

console.log(`Research started: ${interaction.id}`);

while (true) {
    const result = await client.interactions.get(interaction.id);
    if (result.status === 'completed') {
        console.log(result.steps.at(-1).content[0].text);
        break;
    } else if (result.status === 'failed') {
        console.log(`Research failed: ${result.error}`);
        break;
    }
    await new Promise(resolve => setTimeout(resolve, 10000));
}
```

### 1.7.3 REST

```
# 1. Start the research task with image input
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "input": [
        {"type": "text", "text": "Analyze the interspecies dynamics and behavioral risks present in the provided image of the African watering hole. Specifically, investigate the symbiotic relationship between the avian species and the pachyderms shown, and conduct a risk assessment for the reticulated giraffes based on their drinking posture relative to the specific predator visible in the foreground."},
        {"type": "image", "mime_type": "image/jpeg", "uri": "https://storage.googleapis.com/generativeai-downloads/images/generated_elephants_giraffes_zebras_sunset.jpg"}
    ],
    "agent": "deep-research-preview-04-2026",
    "background": true
}'

# 2. Poll for results (Replace INTERACTION_ID)
# curl -X GET "https://generativelanguage.googleapis.com/v1beta/interactions/INTERACTION_ID" \
# -H "x-goog-api-key: $GEMINI_API_KEY"
```

### 1.7.4 文档理解

借助文档理解，您可以直接将文档作为多模态输入传递。
智能体会分析提供的文档，并根据其内容进行研究。

### 1.7.5 Python

```python
from google import genai

client = genai.Client()

interaction = client.interactions.create(
    agent="deep-research-preview-04-2026",
    input=[
        {"type": "text", "text": "What is this document about?"},
        {
            "type": "document",
            "uri": "https://arxiv.org/pdf/1706.03762",
            "mime_type": "application/pdf",
        },
    ],
    background=True,
)
```

### 1.7.6 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const interaction = await client.interactions.create({
    agent: 'deep-research-preview-04-2026',
    input: [
        { type: 'text', text: 'What is this document about?' },
        {
            type: 'document',
            uri: 'https://arxiv.org/pdf/1706.03762',
            mime_type: 'application/pdf'
        }
    ],
    background: true
});
```

### 1.7.7 REST

```
# 1. Start the research task with document input
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "agent": "deep-research-preview-04-2026",
    "input": [
        {"type": "text", "text": "What is this document about?"},
        {"type": "document", "uri": "https://arxiv.org/pdf/1706.03762", "mime_type": "application/pdf"}
    ],
    "background": true
}'
```

## 1.8 处理长时间运行的任务

Deep Research 是一个多步骤流程，涉及规划、搜索、阅读和撰写。此周期通常会超出同步 API 调用的标准超时限制。

智能体必须使用 `background=True`。API 会立即返回部分 `Interaction` 对象。您可以使用 `id` 属性检索互动以进行轮询。互动状态将从 `in_progress` 转换为 `completed` 或 `failed`。如需查看有关管理后台任务的全面指南，请参阅[后台执行](https://ai.google.dev/gemini-api/docs/background-execution?hl=zh-cn)。

### 1.8.1 流式传输

Deep Research 支持流式传输，以接收有关研究进度的实时更新，包括思路摘要、文本输出和生成的图片。
您必须设置 `stream=True` 和 `background=True`。

如需接收中间推理步骤（思路）和进度更新，
您必须通过在 `agent_config` 中将 `thinking_summaries` 设置为
`"auto"` 来启用 **思路摘要**。否则，流可能只会提供最终结果。

#### 1.8.1.1 流事件类型

| 事件类型 | 增量类型 | 说明 |
| --- | --- | --- |
| `step.delta` | `thought` | 智能体的中间推理步骤。 |
| `step.delta` | `text` | 最终文本输出的一部分。 |
| `step.delta` | `image` | 生成的图片（base64 编码）。 |

以下示例启动了研究任务，并通过自动重新连接处理流。它会跟踪 `interaction_id` 和 `last_event_id`，以便在连接断开（例如在 600 秒超时后）时，可以从中断的位置继续。

### 1.8.2 Python

```python
from google import genai

client = genai.Client()

interaction_id = None
last_event_id = None
is_complete = False

def process_stream(stream):
    global interaction_id, last_event_id, is_complete
    for event in stream:
        if event.event_type == "interaction.created":
            interaction_id = event.interaction.id
        if event.event_id:
            last_event_id = event.event_id
        if event.event_type == "step.delta":
            if event.delta.type == "text":
                print(event.delta.text, end="", flush=True)
            elif event.delta.type == "thought":
                print(f"Thought: {event.delta.text}", flush=True)
        elif event.event_type in ("interaction.completed", "interaction.error"):
            is_complete = True

stream = client.interactions.create(
    input="Research the history of Google TPUs.",
    agent="deep-research-preview-04-2026",
    background=True,
    stream=True,
    agent_config={"type": "deep-research", "thinking_summaries": "auto"},
)
process_stream(stream)

# Reconnect if the connection drops
while not is_complete and interaction_id:
    status = client.interactions.get(interaction_id)
    if status.status != "in_progress":
        break
    stream = client.interactions.get(
        id=interaction_id, stream=True, last_event_id=last_event_id,
    )
    process_stream(stream)
```

### 1.8.3 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

let interactionId;
let lastEventId;
let isComplete = false;

async function processStream(stream) {
    for await (const event of stream) {
        if (event.type === 'interaction.created') {
            interactionId = event.interaction.id;
        }
        if (event.event_id) lastEventId = event.event_id;
        if (event.type === 'step.delta') {
            if (event.delta.type === 'text') {
                process.stdout.write(event.delta.text);
            } else if (event.delta.type === 'thought') {
                console.log(`Thought: ${event.delta.text}`);
            }
        } else if (['interaction.completed', 'interaction.error'].includes(event.type)) {
            isComplete = true;
        }
    }
}

const stream = await client.interactions.create({
    input: 'Research the history of Google TPUs.',
    agent: 'deep-research-preview-04-2026',
    background: true,
    stream: true,
    agent_config: { type: 'deep-research', thinking_summaries: 'auto' },
});
await processStream(stream);

// Reconnect if the connection drops
while (!isComplete && interactionId) {
    const status = await client.interactions.get(interactionId);
    if (status.status !== 'in_progress') break;
    const resumeStream = await client.interactions.get(interactionId, {
        stream: true, last_event_id: lastEventId,
    });
    await processStream(resumeStream);
}
```

### 1.8.4 REST

```
# 1. Start the stream (save the INTERACTION_ID from the interaction.start event
#    and the last "event_id" you receive)
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "input": "Research the history of Google TPUs.",
    "agent": "deep-research-preview-04-2026",
    "background": true,
    "stream": true,
    "agent_config": {
        "type": "deep-research",
        "thinking_summaries": "auto"
    }
}'

# 2. If the connection drops, reconnect with your saved IDs
curl -X GET "https://generativelanguage.googleapis.com/v1beta/interactions/INTERACTION_ID?stream=true&last_event_id=LAST_EVENT_ID" \
-H "x-goog-api-key: $GEMINI_API_KEY"
```

## 1.9 后续问题和互动

在智能体返回最终报告后，您可以使用 `previous_interaction_id` 继续对话。这样，您就可以请求对研究的特定部分进行澄清、总结或详细说明，而无需重新开始整个任务。

### 1.9.1 Python

```python
import time
from google import genai

client = genai.Client()

interaction = client.interactions.create(
    input="Can you elaborate on the second point in the report?",
    model="gemini-3.1-pro-preview",
    previous_interaction_id="COMPLETED_INTERACTION_ID"
)

print(interaction.steps[-1].content[0].text)
```

### 1.9.2 JavaScript

```javascript
const interaction = await client.interactions.create({
    input: 'Can you elaborate on the second point in the report?',
    model: 'gemini-3.1-pro-preview',
    previous_interaction_id: 'COMPLETED_INTERACTION_ID'
});
console.log(interaction.steps.at(-1).content[0].text);
```

### 1.9.3 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "input": "Can you elaborate on the second point in the report?",
    "model": "gemini-3.1-pro-preview",
    "previous_interaction_id": "COMPLETED_INTERACTION_ID"
}'
```

## 1.10 Gemini Deep Research 智能体的适用场景

Deep Research 是一种**智能体** ，而不仅仅是一种模型。它最适合需要“分析师即服务”方法而不是低延迟聊天的场景。

| 功能 | 标准 Gemini 模型 | Gemini Deep Research 智能体 |
| --- | --- | --- |
| **延迟时间** | 秒 | 分钟（异步/后台） |
| **流程** | 生成 -> 输出 | 规划 -> 搜索 -> 阅读 -> 迭代 -> 输出 |
| **输出** | 对话文本、代码、简短摘要 | 详细报告、长篇分析、比较表格 |
| **最适合** | 聊天机器人、提取、创意写作 | 市场分析、尽职调查、文献综述、竞争格局 |

## 1.11 智能体配置

Deep Research 使用 `agent_config` 参数来控制行为。
将其作为包含以下字段的字典传递：

| 字段 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `type` | `string` | 是否必需 | 必须为 `"deep-research"`。 |
| `thinking_summaries` | `string` | `"none"` | 设置为 `"auto"` 可在流式传输期间接收中间推理步骤。设置为 `"none"` 可停用。 |
| `visualization` | `string` | `"auto"` | 设置为 `"auto"` 可启用智能体生成的图表和图片。设置为 `"off"` 可停用。 |
| `collaborative_planning` | `boolean` | `false` | 设置为 `true` 可在研究开始之前启用多轮计划审核。 |

### 1.11.1 Python

```
agent_config = {
    "type": "deep-research",
    "thinking_summaries": "auto",
    "visualization": "auto",
    "collaborative_planning": False,
}

interaction = client.interactions.create(
    agent="deep-research-preview-04-2026",
    input="Research the competitive landscape of cloud GPUs.",
    agent_config=agent_config,
    background=True,
)
```

### 1.11.2 JavaScript

```
const interaction = await client.interactions.create({
    agent: 'deep-research-preview-04-2026',
    input: 'Research the competitive landscape of cloud GPUs.',
    agent_config: {
        type: 'deep-research',
        thinking_summaries: 'auto',
        visualization: 'auto',
        collaborative_planning: false,
    },
    background: true,
});
```

### 1.11.3 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "input": "Research the competitive landscape of cloud GPUs.",
    "agent": "deep-research-preview-04-2026",
    "agent_config": {
        "type": "deep-research",
        "thinking_summaries": "auto",
        "visualization": "auto",
        "collaborative_planning": false
    },
    "background": true
}'
```

## 1.12 适用范围和定价

您可以在 Google AI Studio 和 Gemini API 中使用 Interactions API 访问 Gemini Deep Research 智能体。

定价遵循[随用随付模式](https://ai.google.dev/gemini-api/docs/pricing?hl=zh-cn#pricing-for-agents)，具体取决于底层 Gemini 模型和智能体使用的特定工具。与标准聊天请求（一个请求产生一个输出）不同，Deep Research 任务是一种智能体工作流。单个请求会触发规划、搜索、阅读和推理的自主循环。

### 1.12.1 估算费用

费用因所需的研究深度而异。智能体会自主确定回答提示需要多少阅读和搜索。

- **Deep Research** (`deep-research-preview-04-2026`)：对于需要适度分析的典型查询，智能体可能会使用约 80 个搜索查询、约 25 万个输入 token（约 50-70% 缓存）和约 6 万个输出 token。
 - **估计总价**： 每个任务约 1.00 美元 - 3.00 美元
- **Deep Research Max** (`deep-research-max-preview-04-2026`)：对于深入的竞争格局分析或广泛的尽职调查，智能体可能会使用多达约 160 个搜索查询、约 90 万个输入 token（约 50-70% 缓存）和约 8 万个输出 token。
 - **估计总价**： 每个任务约 3.00 美元 - 7.00 美元

## 1.13 存在安全隐患

让智能体能够访问网络和您的私有文件需要仔细考虑安全风险。

- **使用文件进行提示注入**： 智能体会读取您提供的文件的内容。确保上传的文档（PDF、文本文件）来自可信来源。恶意文件可能包含旨在操纵智能体输出的隐藏文本。
- **网络内容风险**： 智能体会搜索公开网络。虽然我们实施了强大的安全过滤条件，但智能体仍有可能遇到并处理恶意网页。我们建议您查看响应中提供的 `citations` 以验证来源。
- **数据泄露**： 如果您还允许智能体浏览网络，请谨慎要求智能体总结敏感的内部数据。

## 1.14 最佳做法

- **提示未知内容**： 指示智能体如何处理缺失的数据。
*例如，在提示中添加“如果 2025 年的具体数据不可用，请明确说明这些数据是预测值或不可用，而不是估算值”。*
- **提供上下文**： 通过在输入提示中直接提供背景信息或限制条件，为智能体的研究提供依据。
- **使用协同规划**： 对于复杂的查询，启用协同规划以在执行之前查看和完善研究计划。
- **多模态输入**： Deep Research 智能体支持多模态输入。
请谨慎使用，因为这会增加费用并增加上下文窗口溢出的风险。

## 1.15 限制

- **自定义工具**： 您目前无法提供自定义函数调用工具，但可以将远程 MCP（模型上下文协议）服务器与 Deep Research 智能体搭配使用。
- **结构化输出**： Deep Research 智能体目前不支持结构化输出。
- **最长研究时间**： Deep Research 智能体的最长研究时间为 60 分钟。大多数任务应在 20 分钟内完成。
- **存储要求**： 使用 `background=True` 执行智能体需要 `store=True`。
- **Google 搜索**： [Google
搜索](https://ai.google.dev/gemini-api/docs/google-search?hl=zh-cn)默认处于启用状态，并且对接地结果有[特定
限制](https://ai.google.dev/gemini-api/terms?hl=zh-cn#use-restrictions2)
。

## 1.16 后续步骤

- 详细了解 [Interactions API](https://ai.google.dev/gemini-api/docs/interactions-overview?hl=zh-cn)。
- 了解如何使用[文件搜索](https://ai.google.dev/gemini-api/docs/file-search?hl=zh-cn)
工具使用您自己的数据。
