# Robotics 工具：Gemini Robotics-ER 原生 generateContent

日期：2026-07-16  
状态：已批准设计（待实现）  
参考：[Gemini Robotics-ER 1.6](https://ai.google.dev/gemini-api/docs/robotics-overview?hl=zh-cn)

## 背景

- 仓库已有通用视觉工具 `vision`（Google Interactions：`describe` / `detect` / `segment`）与多 Agent `orchestration`（串行流水线）。
- Google Robotics-ER 是专用空间推理模型：指点、边界框、轨迹、长任务分解，并可输出对接自定义机器人 API 的函数调用序列。
- 官方示例走原生 `generateContent` + 模型 `gemini-robotics-er-1.6-preview`，与 Interactions / OpenAI 兼容路径不同。
- 目标：新增独立工具，直接使用 Google 原生接口做机器人感知与编排规划（不真连硬件）。

## 目标

1. 新工具 `robotics`：`mode` = `point` | `detect` | `trajectory` | `plan`。
2. Google：**仅** `POST …/v1beta/models/{model}:generateContent`（非 Interactions、非 OpenAI 兼容）。
3. 默认模型 `gemini-robotics-er-1.6-preview`；坐标约定对齐官方文档（点 / 框归一化 0–1000）。
4. `plan`：自然语言步骤 + 指点；可选 `robot_api` 时输出 `[{function, args}]` 序列。

## 非目标

- 真连机械臂 / 执行侧硬件驱动
- code_execution（缩放裁剪等智能体循环）
- 视频跨帧跟踪
- OpenAI / Anthropic 回退
- 前端框 / 轨迹可视化
- 改 ProvidersPanel（首期不加 `robotics_model` 字段）
- 与多 Agent `orchestration.db` 合流（职责不同：此处是物理空间规划，不是 Agent 串行派发）

## 已确认决策

| 项 | 选择 |
|----|------|
| 工具形态 | 独立 `robotics` + `mode`（不扩 `vision`） |
| 首期范围 | 感知（point / detect / trajectory）+ plan |
| Google API | 原生 `generateContent` |
| 失败回退 | 无；仅 Google |
| 模型配置 | 默认固定；工具参数可选 `model` 覆盖；首期不扩 Providers UI |
| 凭证 | `image_gen_targets.google()`（key / base） |

## 工具契约

### 参数

| 字段 | 类型 | 说明 |
|------|------|------|
| `image_urls` | `string[]` | 工作区相对路径或 `http(s)` / `data:`；至少 1 张 |
| `image_url` | `string` | 兼容单图；有则并入 `image_urls` |
| `mode` | enum | `point`（默认）\| `detect` \| `trajectory` \| `plan` |
| `prompt` | `string?` | 缺省用各 mode 内置提示（可覆盖） |
| `queries` | `string[]?` | `point` 可选：要找的对象名列表 |
| `robot_api` | `string?` | `plan` 可选：自定义机器人函数说明；无则只做步骤+指点 |
| `thinking_budget` | `int?` | 未传：感知与 plan 默认 `0` |
| `model` | `string?` | 覆盖默认 `gemini-robotics-er-1.6-preview` |

### 模式语义

- **point**：返回对象点；有 `queries` 时按列表查找，否则场景中最多 10 个物体（对齐官方入门示例）。
- **detect**：2D 边界框列表。
- **trajectory**：起点 + 有序中间点（label 为顺序）。
- **plan**：长任务分解。无 `robot_api` → 分步说明 + 指点；有 → 另附函数调用 JSON 列表。

### 输出约定

| mode | 主载荷 |
|------|--------|
| `point` / `trajectory` | `[{point:[y,x], label}]`，坐标整数 0–1000 |
| `detect` | `[{box_2d:[ymin,xmin,ymax,xmax], label}]`，整数 0–1000 |
| `plan` | 推理/步骤文本 + 可选 `[{function, args}]`；可含指点列表 |

成功输出末尾附元信息行：`provider=google` / `model=` / `mode=`。  
JSON 解析失败时返回原文并标 `parse=raw`，不 panic。

## 协议

### Google generateContent

```http
POST {google_native_base}/v1beta/models/{model}:generateContent
x-goog-api-key: …
Content-Type: application/json
```

请求要点：

- `contents[0].parts`：若干图片 part + 一段 text prompt  
  - 本地 / `data:` → `inlineData.{mimeType,data}`（纯 base64）  
  - `http(s)` → 工具层下载后转 inline（首期不依赖 Files API）
- `generationConfig.temperature`：`1.0`（对齐文档常见示例）
- `generationConfig.thinkingConfig.thinkingBudget`：由参数或 mode 默认写入
- Base：现有 `google_native_base`（剥掉 `/v1beta/openai`）

响应解析：拼接 `candidates[0].content.parts[].text`；工具层剥 code fence 后按 mode 做轻量 JSON 抽取。

### 内置提示（原则）

对齐官方 cookbook 约定，例如：

- point：`[{"point":[y,x],"label":…}]`，`[y,x]` 归一化 0–1000  
- detect：`box_2d` 为 `[ymin,xmin,ymax,xmax]`，仅整数，限对象数  
- trajectory：按轨迹顺序 label `0`…`n`  
- plan：要求步骤说明；若提供 `robot_api`，要求 `[{function,args}]` JSON 列表

具体英文/中文 prompt 字符串在实现计划中固化，并单测断言关键片段。

## 架构

```
image_gen_targets.google()
  → robotics.dispatch
       → resolve images (workspace / http / data)
       → google_robotics_generate   // providers::robotics_http（独立模块，不塞进 Interactions）
       → format text + meta
```

与 `vision` 的边界：

| | `vision` | `robotics` |
|--|----------|------------|
| API | Interactions | generateContent |
| 默认模型 | gemini-3.5-flash | gemini-robotics-er-1.6-preview |
| OpenAI | 可回退 | 无 |
| 职责 | 通用看图 / 检测 / 分割 | 空间指点、轨迹、机器人任务规划 |

禁止 `robotics` 调用 `google_openai_base` / Interactions / OpenAI completions。

## 触及模块

| 区域 | 变更 |
|------|------|
| `crates/agent-providers/src/protocol/robotics_http.rs`（新） | URL、body、解析、`default_robotics_model` |
| `crates/agent-providers/src/protocol/mod.rs`（及 lib 导出） | 挂载模块 |
| `crates/agent-tools/src/builtin/media/robotics.rs`（新） | 注册 + dispatch |
| `crates/agent-tools/src/builtin/media/mod.rs` + 工具 dispatch | 挂载 |
| frontend i18n（`agentTools.robotics`） | 工具描述 |
| 测试 | providers 单测 + tools mock HTTP |

## 错误与边界

| 情况 | 行为 |
|------|------|
| 无 Google Key | 立即失败，提示配置 Google API Key |
| 无图 | 参数错误 |
| 本地文件不存在 | 立即失败 |
| 远程下载失败 | 上浮错误 |
| Google HTTP 非 2xx | 上浮 `error.message`，不回退 |
| JSON 形态异常 | 原文 + `parse=raw` |
| 超大 inline | API 报错上浮（本轮不加硬上限） |

## 默认模型

| 项 | 值 |
|----|-----|
| 默认 | `gemini-robotics-er-1.6-preview` |
| 覆盖 | 工具参数 `model` |

## 验收

- [ ] `point` 返回合法点列表（0–1000）
- [ ] `detect` 返回合法 `box_2d` 列表
- [ ] `trajectory` 返回有序轨迹点
- [ ] `plan` 无 `robot_api`：步骤 + 指点；有：含 `[{function,args}]`
- [ ] 仅 Google 原生 `generateContent`；失败不回退 OpenAI
- [ ] 默认模型为 Robotics-ER 1.6 preview
- [ ] 工具描述写清与 `vision` / 多 Agent orchestration 的区别
- [ ] 无密钥时错误清晰，不 panic

## 风险

| 风险 | 缓解 |
|------|------|
| 预览模型 API / 字段变动 | `model` 可覆盖；解析宽松 |
| 模型偶发非纯 JSON | prompt 强调格式；`parse=raw` |
| 与 `vision.detect` 坐标语义混淆 | 文档与工具描述标明专用模型与用途 |
| 大图撑爆请求体 | 与现网视觉一致先发送；后续可加上限或 Files API |

## 后续（非本轮）

- code_execution 智能体循环
- 视频跟踪
- Providers 面板 `robotics_model` 字段
- 前端叠加显示点 / 框 / 轨迹
- 可选：将 `plan` 的函数序列桥接到真实执行器（需单独安全设计）
