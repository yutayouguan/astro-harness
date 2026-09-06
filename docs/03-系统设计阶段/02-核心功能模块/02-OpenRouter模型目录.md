# OpenRouter 模型目录

> 阶段：系统设计 | 状态：定稿 | 说明：价格、能力、模态，实时同步

数据来源：`https://openrouter.ai/api/v1/models`，价格单位：**美元 / 百万 token**。

---

## API 查询能力

### 基础端点

```bash
# 全量模型（默认只返回文本输出模型）
GET https://openrouter.ai/api/v1/models

# 单个模型详情（别名自动解析）
GET https://openrouter.ai/api/v1/model/{author}/{slug}
# 示例：/api/v1/model/anthropic/claude-sonnet-5
# 变体后缀：/api/v1/model/openai/gpt-4:free

# 模型总数
GET https://openrouter.ai/api/v1/models/count
```

### `output_modalities` — 按输出模态过滤

| 值 | 说明 |
| -- | ---- |
| `text` | 文本输出（默认） |
| `image` | 图像生成模型 |
| `audio` | 音频输出模型 |
| `embeddings` | 嵌入模型 |
| `rerank` | 重排模型 |
| `all` | 全部模型 |

```bash
curl "https://openrouter.ai/api/v1/models?output_modalities=image"
curl "https://openrouter.ai/api/v1/models?output_modalities=text,image"
curl "https://openrouter.ai/api/v1/models?output_modalities=all"
```

Desktop 模型市场使用 `all` 结果，并严格按 `architecture.output_modalities`
将条目分为生成、嵌入和重排三类；不使用模型名称猜测类型。嵌入模型可写入
OpenRouter 的独立 `embedding_model` 配置，重排模型在运行时执行链接入前仅作目录展示。

### `supported_parameters` — 按能力过滤

```bash
# 支持 Tool Use 的模型
curl "https://openrouter.ai/api/v1/models?supported_parameters=tools"

# 支持推理模式
curl "https://openrouter.ai/api/v1/models?supported_parameters=reasoning"

# 支持结构化输出
curl "https://openrouter.ai/api/v1/models?supported_parameters=structured_outputs"
```

### `sort` — 排序维度

| 值 | 说明 |
| -- | ---- |
| `pricing-low-to-high` | 最便宜优先（综合加权价格） |
| `pricing-high-to-low` | 最贵优先 |
| `context-high-to-low` | 最大上下文优先 |
| `throughput-high-to-low` | 最高吞吐量优先（p50 tokens/s） |
| `latency-low-to-high` | 最低首字延迟优先（p50 TTFT） |
| `most-popular` | 过去一周 token 处理量最多 |
| `newest` | 最新上线 |

```bash
curl "https://openrouter.ai/api/v1/models?sort=pricing-low-to-high"
curl "https://openrouter.ai/api/v1/models?sort=throughput-high-to-low&supported_parameters=tools"
curl "https://openrouter.ai/api/v1/models?sort=newest"
```

### 分页

```bash
# 默认不分页，返回全量
# 显式分页（limit 最大 1000）
curl "https://openrouter.ai/api/v1/models?offset=0&limit=500"
# 响应中 links.next 包含下一页 URL，null 表示最后一页
```

---

## API 响应 Schema

### 根对象

```json
{
  "data": [ /* Model 对象数组 */ ],
  "total_count": 400,
  "links": {
    "next": "/api/v1/models?offset=500&limit=500"
  }
}
```

### Model 对象字段

| 字段 | 类型 | 说明 |
| ---- | ---- | ---- |
| `id` | string | 模型唯一标识，用于 API 请求 |
| `canonical_slug` | string | 永久不变的 slug |
| `name` | string | 可读名称 |
| `created` | number | 加入 OpenRouter 的 Unix 时间戳 |
| `description` | string | 能力描述 |
| `context_length` | number | 最大上下文窗口（token） |
| `architecture` | Architecture | 技术能力描述 |
| `pricing` | Pricing | 主要 provider 的价格 |
| `top_provider` | TopProvider | 主要 provider 配置 |
| `supported_parameters` | string[] | 支持的 API 参数列表 |
| `benchmarks` | Benchmarks? | 第三方基准测试排名（无数据时省略） |
| `expiration_date` | string? | 废弃日期（null 表示未废弃） |

### Architecture 对象

```typescript
{
  input_modalities:  string[],  // ["file", "image", "text", "audio", "video"]
  output_modalities: string[],  // ["text"] | ["image", "text"] | ["audio"]
  tokenizer:         string,
  instruct_type:     string | null
}
```

### Pricing 对象

所有价格单位为 **USD / token**（×1,000,000 得每百万 token 价格）。

```typescript
{
  prompt:             string,  // 输入 token 单价
  completion:         string,  // 输出 token 单价
  request:            string,  // 每次请求固定费用
  image:              string,  // 图像输入单价
  web_search:         string,  // 联网搜索单价
  internal_reasoning: string,  // 推理 token 单价
  input_cache_read:   string,  // 缓存读取单价
  input_cache_write:  string,  // 缓存写入单价
  overrides:          PricingOverride[]  // 条件价格（见下）
}
```

### PricingOverride — 条件价格

支持两种触发条件：

**长上下文溢价**（超过 token 阈值后涨价）：

```json
{
  "min_prompt_tokens": 200000,
  "prompt": "0.000005",
  "completion": "0.00002"
}
```

**时段定价**（UTC 时间窗口）：

```json
{
  "utc_start": 1630,
  "utc_end": 30,
  "prompt": "0.00000014",
  "completion": "0.00000021"
}
```

### Benchmarks 对象

```typescript
{
  design_arena: [{
    arena:    string,   // "models" | "builders" | "agents"
    category: string,   // "website" | "gamedev" 等
    elo:      number,   // ELO 评分
    win_rate: number,   // 胜率百分比
    rank:     number    // 排名（1 = 最高）
  }]
}
```

### supported_parameters 常见值

| 参数 | 能力 |
| ---- | ---- |
| `tools` | Function Calling |
| `tool_choice` | 工具选择控制 |
| `reasoning` | 内部推理模式 |
| `include_reasoning` | 响应中包含推理过程 |
| `structured_outputs` | JSON Schema 强制输出 |
| `response_format` | 输出格式控制 |
| `max_tokens` | 响应长度限制 |
| `temperature` | 随机性控制 |
| `seed` | 确定性输出 |
| `web_search_options` | 联网搜索 |

---

## 主流模型速查

### Anthropic

| 模型 ID | 输入 $/1M | 输出 $/1M | 上下文 | 输入模态 | Tool |
| ------- | --------: | --------: | -----: | -------- | :--: |
| `anthropic/claude-fable-5` | $10.00 | $50.00 | 1M | text, image, file | ✓ |
| `anthropic/claude-opus-5` | $5.00 | $25.00 | 1M | text, image, file | ✓ |
| `anthropic/claude-opus-5-fast` | $10.00 | $50.00 | 1M | text, image, file | ✓ |
| `anthropic/claude-opus-4.8` | $5.00 | $25.00 | 1M | text, image, file | ✓ |
| `anthropic/claude-sonnet-5` | $2.00 | $10.00 | 1M | text, image, file | ✓ |

### OpenAI

| 模型 ID | 输入 $/1M | 输出 $/1M | 上下文 | Tool |
| ------- | --------: | --------: | -----: | :--: |
| `openai/gpt-5.6-sol` | $5.00 | $30.00 | 1.05M | ✓ |
| `openai/gpt-5.6-terra` | $1.00 | $6.00 | 1.05M | ✓ |
| `openai/gpt-5.6-luna` | $0.10 | $0.60 | 1.05M | ✓ |

### Google

| 模型 ID | 输入 $/1M | 输出 $/1M | 上下文 | 输入模态 | 输出模态 | Tool |
| ------- | --------: | --------: | -----: | -------- | -------- | :--: |
| `google/gemini-3.6-flash` | $1.50 | $7.50 | 1M | text,image,video,file,audio | text | ✓ |
| `google/gemini-3.5-flash` | $1.50 | $9.00 | 1M | text,image,video,file,audio | text | ✓ |
| `google/gemini-3.5-flash-lite` | $0.30 | $2.50 | 1M | text,image,video,file,audio | text | ✓ |
| `google/gemini-3-pro-image` | $2.00 | $12.00 | 128K | image,text | **image,text** | ✓ |
| `google/gemini-3.1-flash-image` | $0.50 | $3.00 | 128K | image,text | **image,text** | — |

### DeepSeek

| 模型 ID | 输入 $/1M | 输出 $/1M | 上下文 | Tool |
| ------- | --------: | --------: | -----: | :--: |
| `deepseek/deepseek-v4-flash-0731` | $0.09 | $0.18 | 1M | ✓ |

### Qwen

| 模型 ID | 输入 $/1M | 输出 $/1M | 上下文 | 输入模态 | Tool |
| ------- | --------: | --------: | -----: | -------- | :--: |
| `qwen/qwen3.8-max` | $2.00 | $6.00 | 1M | text,image,video | ✓ |
| `qwen/qwen3.7-plus` | $0.32 | $1.28 | 1M | text,image | ✓ |
| `qwen/qwen3.7-flash` | $0.03 | $0.13 | 1M | text,image,video | ✓ |

### xAI

| 模型 ID | 输入 $/1M | 输出 $/1M | 上下文 | Tool |
| ------- | --------: | --------: | -----: | :--: |
| `x-ai/grok-4.5` | $2.00 | $6.00 | 500K | ✓ |

### 免费模型

| 模型 ID | 上下文 | 输入模态 |
| ------- | -----: | -------- |
| `nvidia/nemotron-3-ultra-550b-a55b:free` | 1M | text |
| `poolside/laguna-s-2.1:free` | 256K | text |
| `inclusionai/ling-3.0-flash:free` | 256K | text |
| `cohere/north-mini-code:free` | 256K | text |

---

## 多模态能力速查

| 能力 | 代表模型 |
| ---- | -------- |
| 视频输入 | Gemini 3.5/3.6 Flash、Qwen3.8 Max、MiniMax M3、Step 3.7 Flash |
| 音频输入 | Gemini 3.5/3.6 Flash、Inkling、Meta Muse Spark |
| 图像输出 | Gemini 3 Pro/3.1 Flash Image（目前唯一支持） |
| 文件输入 | Claude 系列、GPT-5.6 系列、Grok、Gemini Flash |
| 嵌入模型 | `output_modalities` 包含 `embeddings` |
| 重排模型 | `output_modalities` 包含 `rerank` |

---

## 与 agent-providers 集成

```toml
# configs/providers.toml

[providers.openrouter]
api_key_env = "OPENROUTER_API_KEY"
base_url    = "https://openrouter.ai/api/v1"
# 兼容 OpenAI API 格式，复用 openai provider 实现
# 模型 ID 使用 OpenRouter 格式："{author}/{slug}"
```

> 模型列表持续更新，建议运行时动态拉取，不要硬编码
