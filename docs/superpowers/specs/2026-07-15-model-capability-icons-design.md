# ModelPicker 能力图标

日期：2026-07-15  
状态：设计已确认，待实现计划

## 背景

聊天顶栏 `ModelPicker` 下列表只展示模型 id 与供应商名，用户无法一眼看出该模型能否调工具、推理、看图、生图/视频/音频。

仓库已有模型能力位管线：

- 后端：`ModelCapabilities { vision, web, reasoning, tools }`，由 API hints + LiteLLM 表 enrich（`model_meta.rs` / `litellm_meta.rs`），**不做模型名硬编码猜测**。
- 前端：`types.ModelCapabilities` 对齐；`ModelPicker` 加载选项时丢弃了 `capabilities`，故列表未展示。

用户确认需求：在模型下拉里展示能力；布局与数据源如下。

## 目标

1. 在 **ModelPicker 下拉列表每一行** 显示该模型已支持的能力小图标。
2. 扩展能力位，覆盖生图 / 生视频 / 生音频，并尽量从 LiteLLM / API 取齐。
3. 悬停（及无障碍标签）显示中/英可读说明。
4. 保持少标优于误标：无来源则为 `false`，不画图标。

## 非目标

- 顶栏 trigger（当前已选模型）不显示能力图标。
- 不引入模型名启发式作为列表主数据源（与现有 `model_meta` 策略一致）。
- 不在本轮大改 Providers 面板或允许用户手改能力位。
- 不改变选模型 / 编辑 prefs 的交互逻辑。

## 已确认决策

| 项 | 选择 |
|----|------|
| 展示形式 | 小图标 + 悬停文案（非文字 chip） |
| 数据来源 | 扩后端能力位，LiteLLM / API enrich（方案 C / 实现路径 1） |
| 出现位置 | 仅下拉列表 |
| 行内布局 | **B**：供应商名同一行右侧排图标 |
| 图标顺序 | 工具 → 推理 → 视觉 → 联网 → 生图 → 视频 → 音频 |

## 数据模型

前后端对齐扩展：

```ts
type ModelCapabilities = {
  tools: boolean
  reasoning: boolean
  vision: boolean
  web: boolean
  image_gen: boolean   // 新增
  video_gen: boolean   // 新增
  audio_gen: boolean   // 新增
}
```

### Enrich 规则（后端）

沿用「本次 API + LiteLLM 重算、清掉旧 heuristic」：

1. **Chat 类**（`mode` 非 embed/image/audio/moderation/video 等非对话模式）：  
   - 或上既有：`supports_function_calling` → `tools`，`supports_reasoning` → `reasoning`，`supports_vision` → `vision`，`supports_web_search` → `web`。  
   - 媒体位默认 false，除非 LiteLLM 另有显式 `supports_image_generation` / 视频相关 / `supports_audio_output` 等字段为真。

2. **媒体 / 非 chat `mode`**：  
   - 继续清零 tools / reasoning / vision / web。  
   - `mode` 含 `image` / `image_generation` → `image_gen = true`。  
   - `mode` 含 `video` → `video_gen = true`。  
   - `mode` 含 `audio`（含 speech / tts 等）或 `supports_audio_output` → `audio_gen = true`。

3. API `supported_methods` 等既有逻辑保留；不因本功能做模型 id 字符串猜测。

4. 查不到 LiteLLM / 无 hints → 各位默认 `false`。

## UI 设计

每行结构（布局 B）：

```
[品牌图标]  gemini-2.0-flash [努力徽章?]     [✓?] [编辑]
            Google  🔧 💡 👁 🌐 …
```

细则：

- 图标约 14–16px，优先 lucide，灰色弱对比，不抢模型名。
- 只渲染为 `true` 的位；全 false 时不占位。
- `title` + `aria-label` 走 i18n（中/英）。
- 努力度徽章仍在模型名旁，与能力图标分区。
- 顶栏 trigger 不变。

## 前端改动

| 位置 | 改动 |
|------|------|
| `frontend/src/types.ts` | `ModelCapabilities` 增加三字段 |
| `ModelPicker` | `ModelOption` 保留 `capabilities`；供应商行渲染图标 |
| 新小组件（如 `ModelCapabilityIcons`） | 按固定顺序渲染，便于单测 |
| `modelCaps.ts` `inferModelCapabilities` | 新字段默认 false（或仅作调用方补缺），**列表主路径以后端 enrich 为准** |
| `messages.ts` | 7 个能力说明文案键 |
| 受影响测试 / 字面量 | 补齐新字段，避免类型缺省 |

## 后端改动

| 位置 | 改动 |
|------|------|
| `model_meta.rs` `ModelCapabilities` | 增加 `image_gen` / `video_gen` / `audio_gen` |
| `litellm_meta.rs` | 解析更多字段；`LiteLlmEntry` 暴露给 enrich 使用 |
| `enrich_model_info` | 按上文规则写入新媒体位；调整 non-chat 分支 |
| 单测 | mode / supports_* fixture → 各位断言 |

## 测试与验收

- Rust：LiteLLM fixture — chat 模型 tools/vision；`image_generation` mode → `image_gen`；audio mode / `supports_audio_output` → `audio_gen`；非 chat 不清反或误保留 tools。
- 前端：图标组件 — 仅渲染 true；顺序固定；全 false → 空。
- 手动：打开 ModelPicker，有 LiteLLM 缓存的供应商下列表可见图标；悬停文案正确；编辑侧栏与选中行为不回归。

## 风险

| 风险 | 缓解 |
|------|------|
| LiteLLM 对部分厂商覆盖不全 | 少标；不硬猜 id |
| 缓存的旧模型 JSON 缺新字段 | serde `default` → false |
| 行内图标挤占供应商文案 | 小图标 + wrap；过窄时自然换行 |
| `inferModelCapabilities` 与后端不一致 | 列表只用后端/缓存 `ModelInfo.capabilities` |

## 验收清单

- [ ] 能力位前后端类型对齐，含三新媒体字段。
- [ ] LiteLLM / API enrich 写入新字段，有单测。
- [ ] ModelPicker 列表按布局 B 显示图标，顶栏无图标。
- [ ] i18n 悬停文案中英齐全。
- [ ] 选模型 / 编辑 prefs 行为无回归。
