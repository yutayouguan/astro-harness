# 1 优先推理 (Priority Inference)

> 来源：[Google AI for Developers](https://ai.google.dev/gemini-api/docs/priority-inference?hl=zh-cn)  
> 抓取说明：官方文档 **Interactions API** 版本（页面默认版本）。  
> 原文标题：优先级推理

---

Gemini Priority API 是一种高级推理层级，专为需要低延迟和最高可靠性的业务关键型工作负载而设计，价格较高。优先层级的流量优先于标准 API 和灵活层级的流量。

优先级推理功能适用于所有 Interactions API 端点。

## 1.1 如何使用“优先级”

如需使用“优先”层级，请将请求中的 `service_tier` 字段设置为 `priority`。如果省略此字段，则默认层级为标准层级。

### 1.1.1 Python

```python
from google import genai

client = genai.Client()

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input="Triage this critical customer support ticket immediately.",
    service_tier='priority'
)
print(interaction.output_text)
```

### 1.1.2 JavaScript

```javascript
import { GoogleGenAI } from '@google/genai';

const ai = new GoogleGenAI({});

async function main() {
    const interaction = await ai.interactions.create({
        model: "gemini-3.5-flash",
        input: "Triage this critical customer support ticket immediately.",
        service_tier: "priority"
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
    "input": "Triage this critical customer support ticket immediately.",
    "service_tier": "priority"
  }'
```

## 1.2 优先级推理的运作方式

优先级推理会将请求路由到高严重性计算队列，从而为面向用户的应用提供可预测的快速性能。其主要机制是，当流量超出动态限制时，服务器会以优雅的方式降级为标准处理，从而确保应用稳定性，而不是使请求失败。

| 功能 | 优先级 | 标准 | Flex | 批量 |
| --- | --- | --- | --- | --- |
| **价格** | 比标准版高出 75-100% | 全价票 | 5 折 | 5 折 |
| **延迟时间** | 秒 | 秒到分钟 | 分钟（目标时长为 1-15 分钟） | 最长 24 小时 |
| **可靠性** | 高（不易掉毛） | 高 / 中高 | 尽力而为（可舍弃） | 高（针对吞吐量） |
| **接口** | 同步 | 同步 | 同步 | 异步 |

### 1.2.1 主要优势

- **低延迟**：专为面向用户的交互式 AI 工具而设计，可实现秒级响应时间。
- **高可靠性**：流量以最高严重程度处理，且严格不可丢弃。
- **平稳降级**：如果流量峰值超出动态限额，系统会自动将流量降级到标准层级进行处理，而不是处理失败，从而防止服务中断。
- **低摩擦**：使用与标准层级和 Flex 层级相同的同步 `create` 方法。

### 1.2.2 使用场景

优先处理非常适合性能和可靠性至关重要的关键业务工作流。

- **互动式 AI 应用**：客户服务聊天机器人和 Copilot，用户支付高价，希望获得快速、一致的回答。
- **实时决策引擎**：需要高度可靠、低延迟结果的系统，例如实时工单分流或欺诈检测。
- **高级客户功能**：需要为付费客户保证更高服务等级目标 (SLO) 的开发者。

### 1.2.3 速率限制

即使优先级消耗量计入[总体交互式流量速率限制](https://aistudio.google.com/rate-limit?hl=zh-cn)，它也有自己的速率限制。优先级推理的默认速率限制为**模型 / 层级标准速率限制的 0.3 倍**

### 1.2.4 优雅降级逻辑

如果因拥塞而超出优先级限制，溢出请求会**自动且平稳地**降级为标准处理，而不是因 503 或 429 错误而失败。降级后的请求按标准费率计费，而不是按 Priority Premium 费率计费。

### 1.2.5 客户责任

- **响应监控**：开发者应监控 API 响应中的 `x-gemini-service-tier` 标头，以检测请求是否经常降级为 `standard`。
- **重试**：客户端必须针对标准错误（例如 `DEADLINE_EXCEEDED`）实现重试逻辑/指数退避算法。

## 1.3 价格

优先级推理的价格比[标准 API](https://ai.google.dev/gemini-api/docs/pricing?hl=zh-cn) 高出 75-100%，按令牌数计费。

## 1.4 支持的模型

以下模型支持优先推理：

| 模型 | 优先级推理 |
| --- | --- |
| [Gemini 3.5 Flash](https://ai.google.dev/gemini-api/docs/models/gemini-3.5-flash?hl=zh-cn) | ✔️ |
| [Gemini 3.1 Flash-Lite](https://ai.google.dev/gemini-api/docs/models/gemini-3.1-flash-lite?hl=zh-cn) | ✔️ |
| [Gemini 3.1 Pro 预览版](https://ai.google.dev/gemini-api/docs/models/gemini-3.1-pro-preview?hl=zh-cn) | ✔️ |
| [Gemini 3 Flash 预览版](https://ai.google.dev/gemini-api/docs/models/gemini-3-flash-preview?hl=zh-cn) | ✔️ |
| [Gemini 2.5 Pro](https://ai.google.dev/gemini-api/docs/models/gemini-2.5-pro?hl=zh-cn) | ✔️ |
| [Gemini 2.5 Flash](https://ai.google.dev/gemini-api/docs/models/gemini-2.5-flash?hl=zh-cn) | ✔️ |
| [Gemini 2.5 Flash-Lite](https://ai.google.dev/gemini-api/docs/models/gemini-2.5-flash-lite?hl=zh-cn) | ✔️ |

## 1.5 后续步骤

- [灵活推理](https://ai.google.dev/gemini-api/docs/flex-inference?hl=zh-cn)，以降低成本。
- [token](https://ai.google.dev/gemini-api/docs/tokens?hl=zh-cn)：了解 token。
