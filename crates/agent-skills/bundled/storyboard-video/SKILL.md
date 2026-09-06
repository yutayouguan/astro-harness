---
name: storyboard-video
description: Turn a short scene idea into a shot list and generate clips with video_gen (Veo). Use when the user wants 分镜、短剧片段、连续镜头、角色一致视频。单张图/单首歌/配音请用 creative-media。
astro_bundled_rev: 7
---

# 分镜短视频（storyboard-video）

用 Google `video_gen`（Veo）把梗概变成可预览的连续短片素材。本技能**不负责**自动剪辑成片。单镜头试拍、出图、作曲、配音见 **`creative-media`**。

## 前置

- 已启用：`video_gen` + **`image_gen`（强烈建议）**
- 「模型提供商」中启用 Google 并配置 API Key

## 推荐流水线（默认按此执行）

1. **收集**：主题、段数、画幅（`16:9` / `9:16`）、风格（`cinematic` / `creative`）、`negative_prompt`。关键方向不清时先用 `ask_user`；每镜 `prompt` 写充实运镜描述，并为片段设短中文 `title`。
2. **分镜表**：镜号｜景别｜动作｜对白｜秒数｜备注。高级镜头时长一律 **8**。
3. **先出图再出视频（默认）**：
   - 用 `image_gen` 生成首帧（及可选尾帧）→ 得到相对路径如 `generated/images/img-….png`
   - 再 `video_gen`：`image=…`，转场加 `last_frame=…`
   - **不要**把 `reference_images` 与 `image`/`last_frame` 并用
4. **续拍**：优先把上一段 `generated/videos/…` 相对路径作 `extend_video=`（仍要新的 `prompt`）；也可传 `extend_video_uri=`（原生会先下载再内联）。仅兼容回退时用上一镜返回的 `operation_id` 作 `extend_video_id=`。
5. **汇总**：列出 `generated/videos/…` 路径与每个 `operation_id`；结果里的 `next_shot_hint` 可直接改 prompt 续用。

可用 `generated/videos/progress-latest.txt` 查看当前生成进度（轮询状态）。

## `video_gen` 参数速查

| 参数 | 用途 |
|------|------|
| `prompt` | 画面与运镜（必填） |
| `image` / `last_frame` | 首尾帧（尾帧必须有首帧）；路径来自 `image_gen` |
| `extend_video` | 上一镜本地 mp4 相对路径（续拍首选） |
| `extend_video_uri` | 上一镜 `video_uri`（原生会先下载再内联） |
| `extend_video_id` | 上一镜 `operation_id`（仅 OpenAI 兼容回退） |
| `reference_images` | 最多 3 张参考图（与首尾帧互斥）；旧字段 `reference_image` 会并入 |
| `style` / `aspect_ratio` / `resolution` / `negative_prompt` | 风格与画质（`negative_prompt`/`style` 仅兼容路径） |
| `person_generation` | `allow_adult` / `allow_all` / `dont_allow` |
| `seed` | 可选整数种子 |
| `duration_seconds` | 高级模式强制 8 |

## 约束

- 无 Google Key 则停止并提示配置。
- 一次只推进少量镜头；长等待属正常。
- 交付物是分镜表 + 片段清单，不是成片。
