# 分镜短视频：扩 `video_gen` + 内置 Skill

日期：2026-07-15  
状态：已批准（待实现）  
关联：[Gemini 媒体扩展参数](./2026-07-15-gemini-media-extra-params-design.md)；[OpenAI 兼容视频字段](https://ai.google.dev/gemini-api/docs/openai)

## 背景

- `video_gen` 已支持：`prompt`、`aspect_ratio`、`duration_seconds`、`resolution`、`negative_prompt`。
- 短剧/连续镜头需要：风格、续拍、参考图、首尾帧插值。
- 决策：不新建 toolset / 分镜 Agent；扩现有 `video_gen` + 内置 Skill `storyboard-video`。

## 目标

1. `video_gen` 支持高级可选参数并正确打到 Google OpenAI 兼容 `…/videos`。
2. 仓库内置 Skill，播种到公开 skills 目录，指导分镜→逐镜生成。
3. 返回值保留 `operation_id`，便于 `extend_video_id` 续拍。

## 非目标

- 独立分镜 Agent、`video_pipeline` toolset
- 多张 `reference_images`、自动 ffmpeg 整集拼接
- 出图 Grounding、`safety_settings` 等杂项 `extra_body`

## 已确认决策

| 项 | 选择 |
|----|------|
| 产品形态 | 扩 `video_gen` + Skill（A） |
| 工具挂载 | 同一 toolset，扁平可选参数（1） |
| 高级参数 MVP | style / extend / reference_image / image / last_frame（B） |
| Skill 分发 | 仓库内置模板 → 播种公开 skills（A） |

## 工具契约：`video_gen`（追加）

| 参数 | 类型 | 说明 |
|------|------|------|
| `style` | string? | 如 `cinematic`、`creative` |
| `extend_video_id` | string? | 已有视频 operation id |
| `reference_image` | string? | 工作区相对路径；上传为单张参考图 |
| `image` | string? | 首帧图路径（图生视频） |
| `last_frame` | string? | 尾帧路径；**必须**同时提供 `image` |

校验：

- 路径相对 `workspace_dir`，不存在则失败。
- `last_frame` 无 `image` → 明确错误。
- 图片读入后以 multipart file part 发送（字段名对齐官方：`image`、`last_frame`、`reference_images` 或文档等价名）。

HTTP：在现有 form 上追加文本字段与文件 part；轮询/下载逻辑不变。

成功输出示例：

```text
视频已生成：…/vid-….mp4
provider=google
model=veo-…
operation_id=…
```

## Skill：`storyboard-video`

- 源：`skills/bundled/storyboard-video/SKILL.md`（frontmatter：`name`、`description`）
- 播种：若公开 skills 下尚无该目录，则从打包资源/bundled 路径复制（本地拷贝，不依赖 GitHub）
- 流程（Skill 正文）：
  1. 收集梗概 / 集数 / 画幅
  2. 产出分镜表（镜号、景别、动作、对白、建议时长）
  3. 可选 `image_gen` 定妆 / 场景静帧
  4. 逐镜 `video_gen`（首镜可参考图；后续 `extend_video_id` 或首尾帧）
  5. 汇总路径与 operation id 清单
- 明确不做整集自动剪辑；鼓励用户在文件空间预览

## 触及模块（预期）

| 区域 | 变更 |
|------|------|
| `providers/.../media_http.rs` | form 字段 + 可选文件 part |
| `tools/.../video_gen.rs` | 参数、读图、校验 |
| `useAgentTools.ts` + i18n | 参数列表 / 文案 |
| `skills/bundled/...` + seed | 内置 Skill 与播种 |

## 验收

- [ ] `style` / `extend_video_id` 出现在出站请求（可 mock）
- [ ] 本地 `image` + `last_frame` 校验与上传路径正确
- [ ] `reference_image` 单张可用
- [ ] Skill 播种后出现在 Skills 面板并可启用
- [ ] 无 Google key 时错误清晰

## 风险

| 风险 | 缓解 |
|------|------|
| 官方 multipart 字段名变动 | 集中在 `media_http`；对照文档一次改 |
| 续拍强制 duration=8 | Skill 写明；工具可选软校验提示 |
| 大图撑爆 form | 与现有 vision 类似先发；后续可加上限 |
