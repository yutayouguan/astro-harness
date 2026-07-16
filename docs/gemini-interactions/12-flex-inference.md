# 1 灵活推理 (Flex Inference)

> 来源：[Google AI for Developers](https://ai.google.dev/gemini-api/docs/flex-inference?hl=zh-cn)  
> 抓取说明：官方文档 **Interactions API** 版本（页面默认版本）。  
> 原文标题：Flex 推理

---

Gemini Flex API 是一种推理层，与标准费率相比，可降低 50% 的费用，但延迟时间可变，并且尽力而为地提供可用性。它专为需要同步处理但不需要标准 API 的实时性能的延迟容许型工作负载而设计。

## 1.1 如何使用 Flex

如需使用 Flex 层级，请在请求中将 `service_tier` 指定为 `flex`。默认情况下，如果省略此字段，请求将使用标准层级。

### 1.1.1 Python

```python
from google import genai

client = genai.Client()

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input="Analyze this dataset for trends...",
    service_tier='flex'
)
print(interaction.output_text)
```

### 1.1.2 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

async function main() {
    const interaction = await client.interactions.create({
        model: 'gemini-3.5-flash',
        input: 'Analyze this dataset for trends...',
        service_tier: 'flex'
    });
    console.log(interaction.output_text);
}
await main();
```

### 1.1.3 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
  -H "Content-Type: application/json" \
  -H "x-goog-api-key: $GEMINI_API_KEY" \
  -d '{
      "model": "gemini-3.5-flash",
      "input": "Analyze this dataset for trends...",
      "service_tier": "flex"
  }'
```

## 1.2 Flex 推理的工作原理

Gemini Flex 推理弥合了标准 API 和 24 小时
周转时间之间的差距，[Batch API](https://ai.google.dev/gemini-api/docs/batch-api?hl=zh-cn)。它利用非高峰时段的“可舍弃”计算容量，为后台任务和顺序工作流提供经济高效的解决方案。

| 功能 | Flex | 优先级 | 标准 | 批量 |
| --- | --- | --- | --- | --- |
| **价格** | 50% 折扣 | 比标准价格高 75-100% | 全价票 | 50% 折扣 |
| **延迟时间** | 分钟（目标为 1-15 分钟） | 低（秒） | 秒到分钟 | 最长 24 小时 |
| **可靠性** | 尽力而为（可舍弃） | 高（不可舍弃） | 高 / 中高 | 高（针对吞吐量） |
| **接口** | 同步 | 同步 | 同步 | 异步 |

### 1.2.1 主要优势

- **经济高效**：大幅节省非生产评估、后台代理和数据扩充的费用。
- **低摩擦**：只需向现有请求添加一个参数即可。
- **同步工作流**：非常适合顺序 API 链，其中下一个请求取决于上一个请求的输出，因此与代理工作流的批量处理相比，它更灵活。

### 1.2.2 使用场景

- **离线评估**：运行“LLM 即法官”回归测试或排行榜。
- **后台代理**：顺序任务，例如 CRM 更新、个人资料构建或内容审核，其中可以接受几分钟的延迟。
- **预算受限的研究**：学术实验，需要在有限的预算下使用大量 token。

### 1.2.3 速率限制

Flex 推理流量计入一般 [速率限制](https://aistudio.google.com/rate-limit?hl=zh-cn)；它不
提供像 [Batch API](https://ai.google.dev/gemini-api/docs/batch-api?hl=zh-cn) 那样的扩展速率限制。

### 1.2.4 可舍弃的容量

Flex 流量的处理优先级较低。如果标准流量激增，Flex 请求可能会被抢占或逐出，以确保高优先级用户的容量。如果您需要高优先级的推理，请查看
[优先级推理](https://ai.google.dev/gemini-api/docs/priority-inference?hl=zh-cn)

### 1.2.5 错误代码

当 Flex 容量不可用或系统拥塞时，API 将返回标准错误代码：

- **503 服务不可用**：系统目前已达配额上限。
- **429 请求过多**：速率限制或资源耗尽。

### 1.2.6 客户端责任

- **无服务器端回退**：为防止意外收费，如果 Flex 容量已满，系统不会
自动将 Flex 请求升级到标准层级。
- **重试**：您必须使用
指数退避算法实现自己的客户端重试逻辑。
- **超时**：由于 Flex 请求可能会排队，因此我们建议
将客户端超时时间增加到 10 分钟或更长时间，以避免过早
关闭连接。

## 1.3 调整超时窗口

您可以为 REST API 和客户端库配置每个请求的超时时间。
请务必确保客户端超时时间涵盖预期的服务器等待窗口（例如，Flex 等待队列为 600 秒以上）。SDK 需要以毫秒为单位的超时值。

### 1.3.1 每个请求的超时时间

### 1.3.2 Python

```python
from google import genai

client = genai.Client(http_options={"timeout": 900000})

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input="why is the sky blue?",
    service_tier="flex",
)
```

### 1.3.3 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const client = new GoogleGenAI({});

async function main() {
    const interaction = await client.interactions.create({
        model: "gemini-3.5-flash",
        input: "why is the sky blue?",
        service_tier: "flex",
    }, {timeout: 900000});
}

await main();
```

## 1.4 实现重试

由于 Flex 是可舍弃的，并且会因 503 错误而失败，因此以下示例展示了如何选择性地实现重试逻辑以继续处理失败的请求：

### 1.4.1 Python

```python
import time
from google import genai

client = genai.Client()

def call_with_retry(max_retries=3, base_delay=5):
    for attempt in range(max_retries):
        try:
            return client.interactions.create(
                model="gemini-3.5-flash",
                input="Analyze this batch statement.",
                service_tier="flex",
            )
        except Exception as e:
            if attempt < max_retries - 1:
                delay = base_delay * (2 ** attempt) # Exponential Backoff
                print(f"Flex busy, retrying in {delay}s...")
                time.sleep(delay)
            else:
                print("Flex exhausted, falling back to Standard...")
                return client.interactions.create(
                    model="gemini-3.5-flash",
                    input="Analyze this batch statement."
                )

interaction = call_with_retry()
print(interaction.output_text)
```

### 1.4.2 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const ai = new GoogleGenAI({});

async function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

async function callWithRetry(maxRetries = 3, baseDelay = 5) {
  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      console.log(`Attempt ${attempt + 1}: Calling Flex tier...`);
      const interaction = await ai.interactions.create({
        model: "gemini-3.5-flash",
        input: "Analyze this batch statement.",
        service_tier: 'flex',
      });
      return interaction;
    } catch (e) {
      if (attempt < maxRetries - 1) {
        const delay = baseDelay * (2 ** attempt);
        console.log(`Flex busy, retrying in ${delay}s...`);
        await sleep(delay * 1000);
      } else {
        console.log("Flex exhausted, falling back to Standard...");
        return await ai.interactions.create({
          model: "gemini-3.5-flash",
          input: "Analyze this batch statement.",
        });
      }
    }
  }
}

async function main() {
    const interaction = await callWithRetry();
    console.log(interaction.output_text);
}

await main();
```

## 1.5 价格

Flex 推理的价格为 [标准 API](https://ai.google.dev/gemini-api/docs/pricing?hl=zh-cn) 的 50%
，并按 token 计费。

## 1.6 支持的模型

以下模型支持 Flex 推理：

| 模型 | Flex 推理 |
| --- | --- |
| [Gemini 3.5 Flash](https://ai.google.dev/gemini-api/docs/models/gemini-3.5-flash?hl=zh-cn) | ✔️ |
| [Gemini 3.1 Flash-Lite](https://ai.google.dev/gemini-api/docs/models/gemini-3.1-flash-lite?hl=zh-cn) | ✔️ |
| [Gemini 3.1 Pro 预览版](https://ai.google.dev/gemini-api/docs/models/gemini-3.1-pro-preview?hl=zh-cn) | ✔️ |
| [Gemini 3 Flash 预览版](https://ai.google.dev/gemini-api/docs/models/gemini-3-flash-preview?hl=zh-cn) | ✔️ |
| [Gemini 2.5 Pro](https://ai.google.dev/gemini-api/docs/models/gemini-2.5-pro?hl=zh-cn) | ✔️ |
| [Gemini 2.5 Flash](https://ai.google.dev/gemini-api/docs/models/gemini-2.5-flash?hl=zh-cn) | ✔️ |
| [Gemini 2.5 Flash-Lite](https://ai.google.dev/gemini-api/docs/models/gemini-2.5-flash-lite?hl=zh-cn) | ✔️ |

## 1.7 后续步骤

- [优先级推理](https://ai.google.dev/gemini-api/docs/priority-inference?hl=zh-cn)，实现超低延迟。
- [token](https://ai.google.dev/gemini-api/docs/tokens?hl=zh-cn)：了解 token。
