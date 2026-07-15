---
name: storyboard-video
description: Turn a short scene idea into a shot list and generate clips with video_gen (Veo). Use when the user wants 分镜、短剧片段、连续镜头、角色一致视频。
---

# 分镜短视频（storyboard-video）

用 Google `video_gen`（Veo OpenAI 兼容）把梗概变成可预览的连续短片素材。本技能**不负责**自动剪辑成片。

## 前置

- 已启用工具集：`video_gen`（建议同时开 `image_gen` 做定妆/静帧）
- 「模型提供商」中启用 Google 并配置 API Key

## 工作流程

1. **收集**：主题、集数/段数、画幅（`16:9` / `9:16`）、风格（`cinematic` / `creative`）、禁出现内容（`negative_prompt`）。
2. **分镜表**：输出表格——镜号｜景别｜画面动作｜对白/旁白｜建议秒数｜备注。单镜时长优先 **8 秒**（参考图、首尾帧、续拍时 Veo 要求 `duration_seconds=8`）。
3. **定妆（可选）**：用 `image_gen` 生成角色/场景静帧，路径记入分镜表。
4. **逐镜生成**：调用 `video_gen`：
   - 首镜：可用 `reference_image` 或 `image`（+ 可选 `last_frame`）
   - 后续：优先 `extend_video_id=<上一镜 operation_id>`，或换 `image`/`last_frame` 做转场
   - 始终带上清晰 `prompt`；需要时加 `style`、`aspect_ratio`、`resolution`、`negative_prompt`
5. **汇总**：列出每个镜头的文件路径与 `operation_id`，提醒用户在文件空间预览；说明整集拼接可后续手动/别的工具完成。

## `video_gen` 参数速查

| 参数 | 用途 |
|------|------|
| `prompt` | 画面与运镜描述（必填） |
| `aspect_ratio` | `16:9` / `9:16` |
| `duration_seconds` | `4` / `6` / `8`；高级模式默认用 `8` |
| `resolution` | `720p` / `1080p` / `4K` |
| `negative_prompt` | 排除项 |
| `style` | `cinematic` / `creative` |
| `extend_video_id` | 续拍上一镜 |
| `reference_image` | 工作区相对路径，角色/风格参考（1 张） |
| `image` | 首帧路径 |
| `last_frame` | 尾帧路径（必须同时有 `image`） |

## 约束与失败处理

- `last_frame` 不能单独使用。
- 无 Google Key 时停止并提示配置，不要假装已生成。
- 单次生成可能需数分钟；一次只推进少数镜头，完成后把路径反馈给用户再继续。
- 不要承诺「一集短剧成品」；交付物是分镜表 + 片段文件清单。
