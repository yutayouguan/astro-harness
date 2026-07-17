# 1 迁移指南：从 generateContent 到 Interactions API

> 来源：[Google AI for Developers](https://ai.google.dev/gemini-api/docs/migrate-to-interactions?hl=zh-cn)  
> 抓取说明：官方文档 **Interactions API** 版本（页面默认版本）。  
> 原文标题：迁移到 Interactions API

---

本指南可帮助您从 `generateContent` API 改用 Interactions API。

Interactions API 是使用 Gemini 模型和智能体进行构建的最简单、最出色的方式。虽然 `generateContent` 仍完全受支持，但我们建议所有新开发项目都使用 Interactions API。

> [!NOTE]
> **使用编码智能体自动完成此迁移。**如果您使用的编码代理支持技能（例如 Gemini CLI 或 Jules），请安装 [Gemini Interactions API 技能](https://ai.google.dev/gemini-api/docs/coding-agents?hl=zh-cn#gemini-interactions-api)并运行以下命令：
>
> ```
> /gemini-interactions-api migrate my app to the interactions api
> ```
>
> 技能应用了本指南中描述的 API 更改。

### 1.1.1 为什么迁移？

Interactions API 是我们使用 Gemini 模型和智能体进行构建的最简单、最有效的方式：

- **服务器端历史记录管理**：通过 `previous_interaction_id` 简化多轮对话流程。服务器默认启用状态 (`store=true`)，但您可以通过设置 `store=false` 选择无状态行为。
- **可观测的执行步骤**：通过类型化的步骤，可以轻松调试复杂流程，并为中间事件（例如想法或搜索 widget）呈现界面。
- **工具使用和智能体工作流**：通过类型化执行步骤，原生支持多步骤工具使用、编排和复杂的推理流程。
- **长时间运行的任务和后台任务**：支持使用 `background=true` 将耗时的操作（例如深度思考和深度研究）分流到后台进程。

## 1.2 基本输入/输出

本部分展示了如何迁移简单的文本生成请求。

### 1.2.1 之前 (`generateContent`)

`generateContent` API 是无状态的，可直接返回响应。响应结构将输出封装在 `candidates` 列表中，每个 `candidates` 都包含一个 `content`，其中包含要解析的 `parts` 列表。

### 1.2.2 Python

```python
from google import genai

client = genai.Client()

response = client.models.generate_content(
    model="gemini-2.5-flash-lite", contents="Tell me a joke."
)
print(response.text)
```

### 1.2.3 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const ai = new GoogleGenAI({});

const response = await ai.models.generateContent({
  model: "gemini-2.5-flash-lite",
  contents: "Tell me a joke.",
});
console.log(response.text);
```

### 1.2.4 REST

```
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:generateContent" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "contents": [{
        "parts": [{
            "text": "Tell me a joke."
        }]
    }]
}'

# Response
{
  "candidates": [
    {
      "content": {
        "parts": [
          {
            "text": "Why did the chicken cross the road? To get to the other side!"
          }
        ],
        "role": "model"
      },
      "finishReason": "STOP",
      "index": 0
    }
  ],
  "usageMetadata": {
    "promptTokenCount": 4,
    "candidatesTokenCount": 12,
    "totalTokenCount": 16
  }
}
```

Interactions API 会返回具有 `steps` 时间轴的已存储互动资源。虽然您可以手动检查 `steps` 数组来查找中间事件，但 Google GenAI SDK 会在返回的 `Interaction` 对象上直接提供便捷的属性，以便访问最终输出。

最常见的便利属性是 **`.output_text`**（字符串），它会自动提取并连接模型响应末尾的连续 `TextContent` 代码块。虽然这对于简单回答来说非常有效，但它不包括被非文本内容（例如想法、图片、音频或工具调用）分隔的早期文本块。对于复杂或交错的多模态回答，您必须手动迭代 `steps`。

> [!NOTE]
> `.output_image`
> `.output_audio`

### 1.2.5 Python

```python
from google import genai

client = genai.Client()

interaction = client.interactions.create(
    model="gemini-3.5-flash", input="Tell me a joke."
)

print(interaction.output_text)
```

### 1.2.6 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

let interaction = await client.interactions.create({
    model: 'gemini-3.5-flash',
    input: 'Tell me a joke.'
});

console.log(interaction.output_text);
```

### 1.2.7 REST

```bash
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta2/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "model": "gemini-3.5-flash",
    "input": "Tell me a joke."
}'

# Response
{
  "id": "int_123",
  "status": "completed",
  "steps": [
    {
      "type": "user_input",
      "status": "done",
      "content": [
        {
          "type": "text",
          "text": "Tell me a joke."
        }
      ]
    },
    {
      "type": "model_output",
      "status": "done",
      "content": [
        {
          "type": "text",
          "text": "Why did the chicken cross the road?"
        }
      ]
    }
  ]
}
```

## 1.3 多轮对话

默认情况下，Interactions API 会存储互动，从而实现多轮对话的服务器端状态管理。

### 1.3.1 之前 (`generateContent`)

在 `generateContent` 中，您必须使用 `contents` 数组或客户端聊天辅助程序手动管理对话历史记录。

### 1.3.2 Python

**使用聊天帮助程序（推荐）**

```python
from google import genai

client = genai.Client()

chat = client.chats.create(model="gemini-2.5-flash-lite")
response1 = chat.send_message("Hi, my name is Phil.")
print(response1.text)

response2 = chat.send_message("What is my name?")
print(response2.text)
```

**手动管理历史记录**

```python
from google import genai
from google.genai import types

client = genai.Client()

response = client.models.generate_content(
    model="gemini-2.5-flash-lite",
    contents=[
        types.Content(
            role="user", parts=[types.Part.from_text(text="Hi, my name is Phil.")]
        ),
        types.Content(
            role="model",
            parts=[types.Part.from_text(text="Hi Phil, how can I help you?")],
        ),
        types.Content(
            role="user", parts=[types.Part.from_text(text="What is my name?")]
        ),
    ],
)
print(response.text)
```

### 1.3.3 JavaScript

**使用聊天帮助程序（推荐）**

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const chat = client.chats.create({ model: 'gemini-2.5-flash-lite' });
let response = await chat.sendMessage({ message: 'Hi, my name is Phil.' });
console.log(response.text);

response = await chat.sendMessage({ message: 'What is my name?' });
console.log(response.text);
```

**手动管理历史记录**

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const response = await client.models.generateContent({
    model: 'gemini-2.5-flash-lite',
    contents: [
        { role: 'user', parts: [{ text: 'Hi, my name is Phil.' }] },
        { role: 'model', parts: [{ text: 'Hi Phil, how can I help you?' }] },
        { role: 'user', parts: [{ text: 'What is my name?' }] }
    ]
});
console.log(response.text);
```

### 1.3.4 REST

```
# Request (the second turn requires sending the entire history)
curl -X POST "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:generateContent" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "contents": [
        {"role": "user", "parts": [{"text": "Hi, my name is Phil."}]},
        {"role": "model", "parts": [{"text": "Hi Phil, how can I help you?"}]},
        {"role": "user", "parts": [{"text": "What is my name?"}]}
    ]
}'

# Response
{
  "candidates": [
    {
      "content": {
        "parts": [
          {
            "text": "Your name is Phil."
          }
        ],
        "role": "model"
      },
      "finishReason": "STOP",
      "index": 0
    }
  ]
}
```

### 1.3.5 之后（Interactions API）

Interactions API 在服务器上管理状态。您可以通过引用 `previous_interaction_id` 来继续对话。

### 1.3.6 Python

```python
from google import genai

client = genai.Client()

interaction1 = client.interactions.create(
    model="gemini-3.5-flash", input="Hi, my name is Phil."
)
print("Response 1:", interaction1.output_text)

interaction2 = client.interactions.create(
    model="gemini-3.5-flash",
    previous_interaction_id=interaction1.id,
    input="What is my name?",
)
print("Response 2:", interaction2.output_text)
```

### 1.3.7 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

let interaction = await client.interactions.create({
    model: 'gemini-3.5-flash',
    input: 'Hi, my name is Phil.'
});
console.log("Response 1:", interaction.output_text);

interaction = await client.interactions.create({
    model: 'gemini-3.5-flash',
    previous_interaction_id: interaction.id,
    input: 'What is my name?'
});
console.log("Response 2:", interaction.output_text);
```

### 1.3.8 REST

```bash
# First Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta2/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "model": "gemini-3.5-flash",
    "input": "Hi, my name is Phil."
}'

# Second Request (using ID from first response)
curl -X POST "https://generativelanguage.googleapis.com/v1beta2/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "model": "gemini-3.5-flash",
    "previous_interaction_id": "int_123",
    "input": "What is my name?"
}'

# Response to Second Request
{
  "id": "int_123",
  "steps": [
    {
      "type": "user_input",
      "status": "done",
      "content": [{ "type": "text", "text": "Hi, my name is Phil." }]
    },
    {
      "type": "model_output",
      "status": "done",
      "content": [{ "type": "text", "text": "Hello Phil! How can I help you today?" }]
    },
    {
      "type": "user_input",
      "status": "done",
      "content": [{ "type": "text", "text": "What is my name?" }]
    },
    {
      "type": "model_output",
      "status": "done",
      "content": [{ "type": "text", "text": "Your name is Phil." }]
    }
  ]
}
```

## 1.4 多模态输入

这两个 API 都支持多模态输入（文本、图片、视频等）。

### 1.4.1 之前 (`generateContent`)

在 `generateContent` 中，您可以在 `contents` 数组中传递 `parts` 的列表。响应会在第一个候选对象的 `parts` 中返回输出。

### 1.4.2 Python

```python
from google import genai
from google.genai import types

client = genai.Client()

with open("sample.jpg", "rb") as f:
    image_bytes = f.read()

response = client.models.generate_content(
    model="gemini-2.5-flash-lite",
    contents=[
        types.Part.from_bytes(data=image_bytes, mime_type="image/jpeg"),
        "Describe this image.",
    ],
)
print(response.text)
```

### 1.4.3 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';
import * as fs from 'fs';

const client = new GoogleGenAI({});

const imageBytes = fs.readFileSync('sample.jpg').toString('base64');

const response = await client.models.generateContent({
    model: 'gemini-2.5-flash-lite',
    contents: [
        {
            inlineData: {
                data: imageBytes,
                mimeType: 'image/jpeg',
            },
        },
        'Describe this image.',
    ],
});
console.log(response.text);
```

### 1.4.4 REST

```
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:generateContent" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "contents": [{
        "parts": [
            {
                "inlineData": {
                    "mimeType": "image/jpeg",
                    "data": "..."
                }
            },
            {
                "text": "Describe this image."
            }
        ]
    }]
}'

# Response
{
  "candidates": [
    {
      "content": {
        "parts": [
          {
            "text": "This is a picture of a beautiful sunset."
          }
        ],
        "role": "model"
      }
    }
  ]
}
```

### 1.4.5 之后（Interactions API）

在 Interactions API 中，您需要将数组传递给 `input` 字段。您可以在时间轴中找到 `model_output` 步骤，以检索输出内容。

### 1.4.6 Python

```python
import base64
from google import genai

client = genai.Client()

with open("sample.jpg", "rb") as f:
    image_bytes = f.read()
image_b64 = base64.b64encode(image_bytes).decode("utf-8")

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input=[
        {
            "type": "image",
            "mime_type": "image/jpeg",
            "data": image_b64,
        },
        {"type": "text", "text": "Describe this image."},
    ],
)
print(interaction.output_text)
```

### 1.4.7 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';
import * as fs from 'fs';

const client = new GoogleGenAI({});

const imageBytes = fs.readFileSync('sample.jpg').toString('base64');

const interaction = await client.interactions.create({
    model: 'gemini-3.5-flash',
    input: [
        {
            type: 'image',
            mime_type: 'image/jpeg',
            data: imageBytes
        },
        {
            type: 'text',
            text: 'Describe this image.'
        }
    ]
});
console.log(interaction.output_text);
```

### 1.4.8 REST

```bash
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta2/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "model": "gemini-3.5-flash",
    "input": [
        {
            "type": "image",
            "mime_type": "image/jpeg",
            "data": "..."
        },
        {
            "type": "text",
            "text": "Describe this image."
        }
    ]
}'

# Response
{
  "id": "int_multimodal",
  "steps": [
    {
      "type": "user_input",
      "status": "done",
      "content": [
        {
          "type": "image",
          "mime_type": "image/jpeg",
          "data": "..."
        },
        {
          "type": "text",
          "text": "Describe this image."
        }
      ]
    },
    {
      "type": "model_output",
      "status": "done",
      "content": [
        {
          "type": "text",
          "text": "This is a picture of a beautiful sunset over the mountains."
        }
      ]
    }
  ]
}
```

## 1.5 结构化输出

如需让模型返回符合特定架构的 JSON，请配置回答格式。

### 1.5.1 之前 (`generateContent`)

在 `generateContent` 中，您可以使用嵌套在 `config`（或 `generationConfig`）对象内的 `response_mime_type` 和 `response_schema` 字段来配置输出格式。

### 1.5.2 Python

```python
from google import genai
from google.genai import types
from pydantic import BaseModel

client = genai.Client()

class Recipe(BaseModel):
    recipe_name: str
    ingredients: list[str]

response = client.models.generate_content(
    model="gemini-2.5-flash-lite",
    contents="Give me a recipe for chocolate chip cookies.",
    config=types.GenerateContentConfig(
        response_mime_type="application/json",
        response_schema=Recipe,
    ),
)
print(response.text)
```

### 1.5.3 JavaScript

```javascript
import { GoogleGenAI, Type } from '@google/genai';

const ai = new GoogleGenAI({});

const response = await ai.models.generateContent({
    model: 'gemini-2.5-flash-lite',
    contents: 'Give me a recipe for chocolate chip cookies.',
    config: {
        responseMimeType: 'application/json',
        responseSchema: {
            type: Type.OBJECT,
            properties: {
                recipe_name: { type: Type.STRING },
                ingredients: {
                    type: Type.ARRAY,
                    items: { type: Type.STRING },
                },
            },
            required: ['recipe_name', 'ingredients'],
        },
    },
});
console.log(response.text);
```

### 1.5.4 REST

```
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:generateContent" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "contents": [{
        "parts": [{
            "text": "Give me a recipe for chocolate chip cookies."
        }]
    }],
    "generationConfig": {
        "responseMimeType": "application/json",
        "responseSchema": {
            "type": "OBJECT",
            "properties": {
                "recipe_name": { "type": "STRING" },
                "ingredients": {
                    "type": "ARRAY",
                    "items": { "type": "STRING" }
                }
            },
            "required": ["recipe_name", "ingredients"]
        }
    }
}'

# Response
{
  "candidates": [
    {
      "content": {
        "parts": [
          {
            "text": "{\n  \"recipe_name\": \"Chocolate Chip Cookies\",\n  \"ingredients\": [\n    \"1 cup butter\",\n    \"1 cup sugar\",\n    \"2 cups flour\",\n    \"1 cup chocolate chips\"\n  ]\n}"
          }
        ],
        "role": "model"
      }
    }
  ]
}
```

### 1.5.5 之后（Interactions API）

在 Interactions API 中，输出格式控制移至顶级 `response_format` 数组。

### 1.5.6 Python

```python
from google import genai
from pydantic import BaseModel

client = genai.Client()

class Recipe(BaseModel):
    recipe_name: str
    ingredients: list[str]

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input="Give me a recipe for chocolate chip cookies.",
    response_format=[
        {
            "type": "text",
            "mime_type": "application/json",
            "schema": Recipe.model_json_schema(),
        }
    ],
)

print(interaction.output_text)
```

### 1.5.7 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const interaction = await client.interactions.create({
    model: 'gemini-3.5-flash',
    input: 'Give me a recipe for chocolate chip cookies.',
    response_format: [
        {
            type: 'text',
            mime_type: 'application/json',
            schema: {
                type: 'object',
                properties: {
                    recipe_name: { type: 'string' },
                    ingredients: {
                        type: 'array',
                        items: { type: 'string' }
                    }
                },
                required: ['recipe_name', 'ingredients']
            }
        }
    ]
});
console.log(interaction.output_text);
```

### 1.5.8 REST

```bash
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta2/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "model": "gemini-3.5-flash",
    "input": "Give me a recipe for chocolate chip cookies.",
    "response_format": [
        {
            "type": "text",
            "mime_type": "application/json",
            "schema": {
                "type": "OBJECT",
                "properties": {
                    "recipe_name": { "type": "STRING" },
                    "ingredients": {
                        "type": "ARRAY",
                        "items": { "type": "STRING" }
                    }
                },
                "required": ["recipe_name", "ingredients"]
            }
        }
    ]
}'

# Response
{
  "id": "int_structured",
  "steps": [
    {
      "type": "user_input",
      "status": "done",
      "content": [{ "type": "text", "text": "Give me a recipe for chocolate chip cookies." }]
    },
    {
      "type": "model_output",
      "status": "done",
      "content": [
        {
          "type": "text",
          "text": "{\n  \"recipe_name\": \"Chocolate Chip Cookies\",\n  \"ingredients\": [\n    \"1 cup butter\",\n    \"1 cup sugar\",\n    \"2 cups flour\",\n    \"1 cup chocolate chips\"\n  ]\n}"
        }
      ]
    }
  ]
}
```

## 1.6 多模态生成

当生成文本以外的模态内容（例如图片或音频）时，主要区别在于回答如何构建生成的媒体。

### 1.6.1 之前 (`generateContent`)

在 `generateContent` 中，响应直接在候选的 `parts` 中返回生成的媒体，通常以 `inlineData` 中的 base64 数据形式返回。

```
# Response structure concept
{
  "candidates": [
    {
      "content": {
        "parts": [
          {
            "text": "Here is your generated image:"
          },
          {
            "inlineData": {
              "mimeType": "image/jpeg",
              "data": "...base64..."
            }
          }
        ]
      }
    }
  ]
}
```

### 1.6.2 之后（Interactions API）

在 Interactions API 中，生成的媒体会显示为时间轴中 `model_output` 步骤的 `content` 数组中的不同项，从而保持互动的按时间顺序排列的流程。

```
# Response structure concept
{
  "id": "int_123",
  "steps": [
    {
      "type": "model_output",
      "status": "done",
      "content": [
        {
          "type": "text",
          "text": "Here is your generated image:"
        },
        {
          "type": "image",
          "mime_type": "image/jpeg",
          "data": "...base64..." // Or a reference URL in future
        }
      ]
    }
  ]
}
```

这样可确保响应解析与输入和文本输出的处理方式保持一致，即所有内容都是时间轴中的一个步骤。

## 1.7 服务器端工具

Gemini 支持内置的服务器端工具，例如 Google 搜索接地。主要区别在于响应如何表示工具执行。

### 1.7.1 之前 (`generateContent`)

在 `generateContent` 中，服务器端工具在很大程度上是不透明的。您启用该工具，并获得包含单独 `groundingMetadata` 对象的最终回答。至关重要的是，引用不是内嵌的；`groundingSupports` 使用字符索引将文本段映射回 `groundingChunks` 中的网页来源。

### 1.7.2 Python

```python
from google import genai
from google.genai import types

client = genai.Client()

response = client.models.generate_content(
    model="gemini-2.5-flash-lite",
    contents="Who won Euro 2024?",
    config=types.GenerateContentConfig(
        tools=[{"google_search": {}}]
    ),
)

metadata = response.candidates[0].grounding_metadata
if metadata.search_entry_point:
    print(f"Search Entry Point: {metadata.search_entry_point.rendered_content}")

for support in metadata.grounding_supports:
    print(f"Citation: {support.segment.text}")
```

### 1.7.3 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const response = await client.models.generateContent({
    model: 'gemini-2.5-flash-lite',
    contents: 'Who won Euro 2024?',
    config: {
        tools: [{ google_search: {} }]
    }
});

const metadata = response.candidates[0].groundingMetadata;
if (metadata.searchEntryPoint) {
    console.log(`Search Entry Point: ${metadata.searchEntryPoint.renderedContent}`);
}
for (const support of metadata.groundingSupports) {
    console.log(`Citation: ${support.segment.text}`);
}
```

### 1.7.4 REST

```
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:generateContent" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "contents": [{
        "parts": [{
            "text": "Who won Euro 2024?"
        }]
    }],
    "tools": [{
        "googleSearchRetrieval": {}
    }]
}'

# Response
{
  "candidates": [
    {
      "content": {
        "parts": [
          {
            "text": "Spain won Euro 2024, defeating England 2-1 in the final. This victory marks Spain's record fourth European Championship title."
          }
        ],
        "role": "model"
      },
      "groundingMetadata": {
        "webSearchQueries": [
          "UEFA Euro 2024 winner",
          "who won euro 2024"
        ],
        "searchEntryPoint": {
          "renderedContent": "<!-- HTML and CSS for the search widget -->"
        },
        "groundingChunks": [
          {"web": {"uri": "https://vertexaisearch.cloud.google.com.....", "title": "aljazeera.com"}},
          {"web": {"uri": "https://vertexaisearch.cloud.google.com.....", "title": "uefa.com"}}
        ],
        "groundingSupports": [
          {
            "segment": {"startIndex": 0, "endIndex": 85, "text": "Spain won Euro 2024, defeatin..."},
            "groundingChunkIndices": [0]
          },
          {
            "segment": {"startIndex": 86, "endIndex": 210, "text": "This victory marks Spain's..."},
            "groundingChunkIndices": [0, 1]
          }
        ]
      }
    }
  ]
}
```

### 1.7.5 之后（Interactions API）

在 Interactions API 中，服务器端工具可提供完全的时间轴透明度。该 API 会将调用和结果记录为不同的执行 `steps`（`google_search_call` 和 `google_search_result`），从而准确显示模型检索到的数据。

此外，该 API 还会**内嵌**返回引用。`model_output` 步骤中的文本项包含自己的 `annotations` 数组，可直接链接到来源，而无需从单独的元数据对象映射索引。

### 1.7.6 Python

```python
from google import genai

client = genai.Client()

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input="Who won Euro 2024?",
    tools=[{"type": "google_search"}],
)

for step in interaction.steps:
    if step.type == "google_search_result":
        print(f"Search Suggestions: {step.result[0].search_suggestions}")
    elif step.type == "model_output":
        print(f"Answer: {step.content[0].text}")
        if step.content[0].annotations:
            for anno in step.content[0].annotations:
                print(f"Citation: {anno.title} ({anno.uri})")
```

### 1.7.7 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const interaction = await client.interactions.create({
    model: 'gemini-3.5-flash',
    input: 'Who won Euro 2024?',
    tools: [{ type: 'google_search' }]
});

for (const step of interaction.steps) {
    if (step.type === 'google_search_result') {
        console.log(`Search Suggestions: ${step.result[0].search_suggestions}`);
    } else if (step.type === 'model_output') {
        console.log(`Answer: ${step.content[0].text}`);
        if (step.content[0].annotations) {
            for (const anno of step.content[0].annotations) {
                console.log(`Citation: ${anno.title} (${anno.uri})`);
            }
        }
    }
}
```

### 1.7.8 REST

```bash
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta2/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "model": "gemini-3.5-flash",
    "input": "Who won Euro 2024?",
    "tools": [{"type": "google_search"}]
}'

# Response (showing grounding)
{
  "id": "int_grounded",
  "steps": [
    {
      "type": "user_input",
      "status": "done",
      "content": [{ "type": "text", "text": "Who won Euro 2024?" }]
    },
    {
      "type": "google_search_call",
      "status": "done",
      "content": [{ "type": "text", "text": "UEFA Euro 2024 winner" }]
    },
    {
      "type": "google_search_result",
      "status": "done",
      "content": [
        {
          "type": "text",
          "text": "Spain won Euro 2024..." 
        }
      ]
    },
    {
      "type": "model_output",
      "status": "done",
      "content": [
        {
          "type": "text",
          "text": "Spain won Euro 2024, defeating England 2-1.",
          "annotations": [
            {
              "start_index": 0,
              "end_index": 42,
              "uri": "https://vertexaisearch...",
              "title": "aljazeera.com"
            }
          ]
        }
      ]
    }
  ]
}
```

## 1.8 函数调用

函数调用和结果的结构也已更改，以适应步骤架构。

### 1.8.1 之前 (`generateContent`)

在 `generateContent` 中，响应会返回候选对象中的函数调用。* {Python}

```python
```python
from google import genai
from google.genai import types

client = genai.Client()

response = client.models.generate_content(
    model="gemini-2.5-flash-lite",
    contents="What's the weather in Boston?",
    config=types.GenerateContentConfig(tools=[weather_tool]),
)

function_call = response.candidates[0].content.parts[0].function_call
print(f"Requested tool: {function_call.name}")

result = "52°F and rain"

response = client.models.generate_content(
    model="gemini-2.5-flash-lite",
    contents=[
        types.Content(
            role="user",
            parts=[
                types.Part.from_text(text="What's the weather in Boston?")
            ],
        ),
        response.candidates[0].content,
        types.Content(
            role="user",
            parts=[
                types.Part.from_function_response(
                    name=function_call.name,
                    response={"result": result},
                )
            ],
        ),
    ],
    config=types.GenerateContentConfig(tools=[weather_tool]),
)
print(response.text)
```
```

### 1.8.2 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

let response = await client.models.generateContent({
    model: 'gemini-2.5-flash-lite',
    contents: "What's the weather in Boston?",
    config: { tools: [weatherTool] }
});

const functionCall = response.candidates[0].content.parts[0].functionCall;
console.log(`Requested tool: ${functionCall.name}`);

const result = "52°F and rain";

response = await client.models.generateContent({
    model: 'gemini-2.5-flash-lite',
    contents: [
        { role: 'user', parts: [{ text: "What's the weather in Boston?" }] },
        response.candidates[0].content,
        {
            role: 'user',
            parts: [{
                functionResponse: {
                    name: functionCall.name,
                    response: { result: result }
                }
            }]
        }
    ],
    config: { tools: [weatherTool] }
});
console.log(response.text);
```

### 1.8.3 REST

```
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:generateContent" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "contents": [{
        "parts": [{
            "text": "What is the weather like in Boston, MA?"
        }]
    }],
    "tools": [{
        "functionDeclarations": [{
            "name": "get_weather",
            "description": "Get the current weather",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "location": {"type": "STRING"}
                },
                "required": ["location"]
            }
        }]
    }]
}'

# Response
{
  "candidates": [
    {
      "content": {
        "parts": [
          {
            "functionCall": {
              "name": "get_weather",
              "args": { "location": "Boston, MA" }
            }
          }
        ],
        "role": "model"
      },
      "finishReason": "STOP",
      "index": 0
    }
  ]
}
```

### 1.8.4 之后（Interactions API）

工具调用和结果现在是时间轴中的不同步骤。

### 1.8.5 Python

```python
from google import genai

client = genai.Client()

weather_tool = {
    "type": "function",
    "name": "get_weather",
    "description": "Gets weather",
    "parameters": {
        "type": "object",
        "properties": {
            "location": {"type": "string"}
        },
    },
}

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input="What's the weather in Boston?",
    tools=[weather_tool],
)

for step in interaction.steps:
    if step.type == "function_call":
        print(f"Executing {step.name} for {step.arguments}")

        result = "52°F and rain"

        interaction = client.interactions.create(
            model="gemini-3.5-flash",
            previous_interaction_id=interaction.id,
            input=[
                {
                    "type": "function_result",
                    "call_id": step.id,
                    "name": step.name,
                    "result": [{"type": "text", "text": result}],
                }
            ],
        )
        print(interaction.output_text)
```

### 1.8.6 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const weatherTool = {
    type: "function",
    name: "get_weather",
    description: "Get weather for a location",
    parameters: {
        type: "object",
        properties: {
            location: { type: "string" }
        },
        required: ["location"]
    }
};

const interaction = await client.interactions.create({
    model: 'gemini-3.5-flash',
    input: "What's the weather in Boston?",
    tools: [weatherTool]
});

for (const step of interaction.steps) {
    if (step.type === 'function_call') {
        console.log(`Executing ${step.name} for ${JSON.stringify(step.arguments)}`);

        const result = "52°F and rain";

        const nextInteraction = await client.interactions.create({
            model: 'gemini-3.5-flash',
            previous_interaction_id: interaction.id,
            input: [
                {
                    type: 'function_result',
                    call_id: step.id,
                    name: step.name,
                    result: [{ type: 'text', text: result }]
                }
            ]
        });

        console.log(nextInteraction.output_text);
    }
}
```

### 1.8.7 REST

```bash
# Initial Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta2/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "model": "gemini-3.5-flash",
    "input": "What's the weather in Boston?",
    "tools": [{
        "type": "function",
        "name": "get_weather",
        "description": "Get weather for a location",
        "parameters": {
            "type": "object",
            "properties": {
                "location": { "type": "string" }
            },
            "required": ["location"]
        }
    }]
}'

# Response (requires action)
{
  "id": "int_001",
  "status": "requires_action",
  "steps": [
    {
      "type": "user_input",
      "status": "done",
      "content": [
        { "type": "text", "text": "What's the weather in Boston?" }
      ]
    },
    {
      "type": "function_call",
      "status": "waiting",
      "id": "fc_1",
      "name": "get_weather",
      "arguments": { "location": "Boston, MA" }
    }
  ]
}

# Submit Tool Result Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta2/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "model": "gemini-3.5-flash",
    "previous_interaction_id": "int_001",
    "input": {
        "type": "function_result",
        "call_id": "fc_1",
        "name": "get_weather",
        "result": [
            { "type": "text", "text": "52°F with rain" }
        ]
    }
}'

# Final Response
{
  "id": "int_002",
  "status": "completed",
  "steps": [
    {
      "type": "function_result",
      "call_id": "fc_1",
      "name": "get_weather",
      "result": [
        { "type": "text", "text": "52°F with rain" }
      ]
    },
    {
      "type": "model_output",
      "status": "done",
      "content": [
        { "type": "text", "text": "It's 52°F with rain in Boston." }
      ]
    }
  ]
}
```

## 1.9 流式

流式传输的一个主要区别在于，Interactions API 使用的端点与 `generateContent` API 相同，但前者在请求正文中包含 `"stream": true`，而后者需要调用专用端点 (`:streamGenerateContent`)。

此外，流式事件现在使用专用类型来监控互动生命周期，并沿时间轴跟踪执行步骤。

### 1.9.1 之前 (`generateContentStream`)

借助 `generateContent`，您可以接收响应块的流。

### 1.9.2 Python

```python
from google import genai

client = genai.Client()

response = client.models.generate_content_stream(
    model="gemini-2.5-flash-lite", contents="Tell me a story"
)
for chunk in response:
    print(chunk.text, end="")
```

### 1.9.3 JavaScript

```
const responseStream = await client.models.generateContentStream({
    model: 'gemini-2.5-flash-lite',
    contents: 'Tell me a story',
});
for await (const chunk of responseStream) {
    process.stdout.write(chunk.text);
}
```

### 1.9.4 REST

```
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:streamGenerateContent" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "contents": [{
        "parts": [{
            "text": "Tell me a story"
        }]
    }]
}'

# Response stream
event: content.start
data: {"event_type": "content.start", "index": 0, "content": {"type": "thought"}}
event: content.delta
data: {"event_type": "content.delta", "index": 0, "delta": {"type": "thought_summary", "text": "User wants an explanation."}}
event: content.stop
data: {"event_type": "content.stop", "index": 0}
event: content.start
data: {"event_type": "content.start", "index": 1, "content": {"type": "text"}}
event: content.delta
data: {"event_type": "content.delta", "index": 1, "delta": {"type": "text", "text": "Hello"}}
event: content.stop
data: {"event_type": "content.stop", "index": 1}
```

### 1.9.5 之后（Interactions API）

在 Interactions API 中，流式传输使用服务器发送的事件 (SSE) 和专门的增量类型来表示执行步骤。

### 1.9.6 Python

```python
from google import genai

client = genai.Client()

stream = client.interactions.create(
    model="gemini-3.5-flash",
    input="Tell me a story",
    stream=True,
)

for event in stream:
    if event.event_type == "step.delta" and event.delta:
        if getattr(event.delta, "type", None) == "text" and getattr(event.delta, "text", None):
            print(event.delta.text, end="", flush=True)
    elif event.event_type == "interaction.completed":
        print(f"\n\n--- Stream Finished ---")
```

### 1.9.7 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const stream = await client.interactions.create({
    model: 'gemini-3.5-flash',
    input: 'Tell me a story',
    stream: true,
});

for await (const event of stream) {
    if (event.event_type === 'step.delta' && event.delta) {
        if (event.delta.type === 'text' && event.delta.text) {
            process.stdout.write(event.delta.text);
        }
    } else if (event.event_type === 'interaction.completed') {
        console.log('\n\n--- Stream Finished ---');
    }
}
```

### 1.9.8 REST

# SSE 流输出示例
**event: interaction.created
data: {"type": "interaction.created", "interaction": {"id": "int_xyz", "status": "created"}}
event: interaction.in_progress
data: {"type": "interaction.in_progress", "interaction": {"id": "int_xyz", "status": "in_progress"}}
event: step.start
data: {"type": "step.start", "index": 0, "step": {"type": "thought"}}
event: step.delta
data: {"type": "step.delta", "index": 0, "delta": {"type": "thought", "text": "User wants an explanation."}}
event: step.stop
data: {"type": "step.stop", "index": 0, "status": "done"}
event: step.start
data: {"type": "step.start", "index": 1, "step": {"type": "model_output"}}
event: step.delta
data: {"type": "step.delta", "index": 1, "delta": {"type": "text", "text": "Hello"}}
event: step.stop
data: {"type": "step.stop", "index": 1, "status": "done"}
event: interaction.completed
data: {"type": "interaction.completed", "interaction": {"id": "int_xyz", "status": "completed", "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}}}**
```

### 1.9.9 流式工具和函数调用

信息流中工具的行为方式已从 `generateContent` 发生显著变化，可提供更精细的控制和可见性。

#### 1.9.9.1 之前 (`generateContent`)

使用 `generateContent` 时，流式函数调用会以单个块的形式完整到达。您无法实时看到正在生成的实参，因此处理程序只是检查是否存在完整的 `functionCall` 对象。

### 1.9.10 Python

```python
from google import genai
from google.genai import types

client = genai.Client()

stream = client.models.generate_content_stream(
    model="gemini-2.5-flash-lite",
    contents="What's the weather in Boston?",
    config=types.GenerateContentConfig(tools=[weather_tool]),
)

for chunk in stream:
    # Function calls arrived complete — no partial arguments
    if chunk.candidates[0].content.parts[0].function_call:
        fc = chunk.candidates[0].content.parts[0].function_call
        print(f"Call: {fc.name}({fc.args})")
    elif chunk.text:
        print(chunk.text, end="")
```

### 1.9.11 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const stream = await client.models.generateContentStream({
    model: 'gemini-2.5-flash-lite',
    contents: "What's the weather in Boston?",
    config: { tools: [weatherTool] }
});

for await (const chunk of stream) {
    const part = chunk.candidates[0].content.parts[0];
    if (part.functionCall) {
        console.log(`Call: ${part.functionCall.name}(${JSON.stringify(part.functionCall.args)})`);
    } else if (part.text) {
        process.stdout.write(part.text);
    }
}
```

### 1.9.12 REST

```
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:streamGenerateContent" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "contents": [{"parts": [{"text": "What is the weather in Boston?"}]}],
    "tools": [{"functionDeclarations": [{"name": "get_weather", "parameters": {"type": "OBJECT", "properties": {"location": {"type": "STRING"}}}}]}]
}'

# Response stream — function call arrives complete in one chunk
{"candidates": [{"content": {"parts": [{"functionCall": {"name": "get_weather", "args": {"location": "Boston, MA"}}}]}}]}
```

#### 1.9.12.1 之后（Interactions API）

Interactions API 会以字符为单位将函数调用实参作为 `arguments` 事件进行流式传输。整个工具生命周期（思考、调用、结果和输出）以一系列不同的步骤呈现。

### 1.9.13 Python

```python
from google import genai

client = genai.Client()

stream = client.interactions.create(
    model="gemini-3.5-flash",
    input="What's the weather in Boston?",
    tools=[get_weather_tool],
    stream=True,
)

for event in stream:
    if event.event_type == "step.start" and event.step:
        if getattr(event.step, "type", None) == "function_call":
            print(f"Calling: {event.step.name}")
    elif event.event_type == "step.delta" and event.delta:
        if getattr(event.delta, "type", None) == "arguments":
            print(f"  args: {event.delta.partial_arguments}")
        elif getattr(event.delta, "type", None) == "text" and getattr(event.delta, "text", None):
            print(event.delta.text, end="")
    elif event.event_type == "interaction.completed":
        print("\n--- Done ---")
```

### 1.9.14 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

const stream = await client.interactions.create({
    model: 'gemini-3.5-flash',
    input: "What's the weather in Boston?",
    tools: [getWeatherTool],
    stream: true,
});

for await (const event of stream) {
    if (event.event_type === 'step.start' && event.step) {
        if (event.step.type === 'function_call') {
            console.log(`Calling: ${event.step.name}`);
        }
    } else if (event.event_type === 'step.delta' && event.delta) {
        if (event.delta.type === 'arguments' && event.delta.partial_arguments) {
            console.log(`  args: ${event.delta.partial_arguments}`);
        } else if (event.delta.type === 'text' && event.delta.text) {
            process.stdout.write(event.delta.text);
        }
    } else if (event.event_type === 'interaction.completed') {
        console.log('\n--- Done ---');
    }
}
```

### 1.9.15 REST

```bash
# Request
curl -X POST "https://generativelanguage.googleapis.com/v1beta2/interactions" \
-H "Content-Type: application/json" \
-H "x-goog-api-key: $GEMINI_API_KEY" \
-d '{
    "model": "gemini-3.5-flash",
    "input": "What is the weather in Boston?",
    "tools": [{"type": "function", "name": "get_weather", "parameters": {"type": "object", "properties": {"location": {"type": "string"}}}}],
    "stream": true
}'

# Response stream
// Interaction created
event: interaction.created
data: {"event_type": "interaction.created", "interaction": {"id": "int_xyz", "status": "in_progress", "object": "interaction", "model": "gemini-3.5-flash"}}

event: interaction.status_update
data: {"event_type": "interaction.status_update", "interaction_id": "int_xyz", "status": "in_progress"}

// ── Function Call（线上 wire：arguments 经 arguments_delta 下发；step 可带 signature）──
event: step.start
data: {"event_type": "step.start", "index": 0, "step": {"type": "function_call", "id": "fc_1", "name": "get_weather", "arguments": {}, "signature": "…"}}

event: step.delta
data: {"event_type": "step.delta", "index": 0, "delta": {"type": "arguments_delta", "arguments": "{\"location\": \"Boston, MA\"}"}}

event: step.stop
data: {"event_type": "step.stop", "index": 0}

// 工具调用暂停：线上为 interaction.completed + status=requires_action（偶见独立 requires_action 事件）
event: interaction.completed
data: {"event_type": "interaction.completed", "interaction": {"id": "int_xyz", "status": "requires_action", "usage": {"total_input_tokens": 66, "total_output_tokens": 16, "total_thought_tokens": 51, "total_cached_tokens": 0, "total_tokens": 133}}}

// ── (Client submits the tool result) ──────────────────
// 客户端以 previous_interaction_id + function_result 续写，再消费后续流。

event: interaction.status_update
data: {"event_type": "interaction.status_update", "interaction_id": "int_xyz", "status": "in_progress"}

// ── Function Result (echoed back, no deltas) ─
event: step.start
data: {"event_type": "step.start", "index": 1, "step": {"type": "function_result", "call_id": "fc_1", "name": "get_weather", "result": [{"type": "text", "text": "52°F, rain"}]}}

event: step.stop
data: {"event_type": "step.stop", "index": 1}

// ── Thought（可选）──────────────────────────────────
event: step.start
data: {"event_type": "step.start", "index": 2, "step": {"type": "thought"}}

event: step.delta
data: {"event_type": "step.delta", "index": 2, "delta": {"type": "thought", "text": "Got weather data. Composing the final response."}}

event: step.stop
data: {"event_type": "step.stop", "index": 2}

// ── Model Output (text streamed) ─────────────
event: step.start
data: {"event_type": "step.start", "index": 3, "step": {"type": "model_output"}}

event: step.delta
data: {"event_type": "step.delta", "index": 3, "delta": {"type": "text", "text": "It's currently 52°F and rainy in Boston."}}

event: step.stop
data: {"event_type": "step.stop", "index": 3}

// ── Interaction complete ─────────────────────────────
event: interaction.completed
data: {"event_type": "interaction.completed", "interaction": {"id": "int_xyz", "status": "completed", "usage": {"total_input_tokens": 256, "total_output_tokens": 128, "total_tokens": 384}}}

event: done
data: [DONE]
```

> **Wire 说明（2026-05-20 Api-Revision）**：参数增量字段为 `delta.type=arguments_delta` + `delta.arguments`（文档旧示例写 `type=arguments` / `partial_arguments`，实现需两者兼容）。用量字段为 `total_input_tokens` / `total_output_tokens` / `total_thought_tokens` / `total_cached_tokens`。`function_call` 的 `signature` 在无状态回放或丢弃 `previous_interaction_id` 时必须原样回传。

