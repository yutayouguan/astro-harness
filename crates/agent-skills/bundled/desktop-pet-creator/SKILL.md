---
name: desktop-pet-creator
description: 从用户描述或上传的宠物照片生成专属 Astro 桌面宠物并立即应用。用户说“生成桌宠”“把这张照片做成桌面宠物”“换一个桌面伙伴”“显示/隐藏桌宠”等请求时使用；普通图片创作不使用。
astro_bundled_rev: 2
astro_tools: [request_user_input_async, image_gen, desktop_pet]
---

# Astro 桌面宠物

把用户的自然语言或宠物照片转换成适合透明悬浮窗口展示的桌面宠物，并通过 `desktop_pet` 应用。不要用 shell 修改 `~/.astro/ui/desktop-pet`。

`image_gen` 和 `desktop_pet` 是延迟工具；当前步骤未显示时，先用 `tool_search` 搜索并加载，再调用。

## 确认创作方向

- 用户提供照片时，把照片作为身份基准，保留物种、脸型、毛色、花纹、眼睛和显著特征。
- 没有照片时，可按文字直接设计原创桌宠。
- 缺少核心风格且无法推断时，只用一次 `request_user_input_async` 询问；优先提供“软萌 3D”“手绘贴纸”“像素精灵”三个选项。用户说“你决定”“随便”“惊喜我”时自行选择。
- 用户只是讨论想法、没有要求生成或应用时，不调用生成工具。

## 生成要求

调用 `image_gen`，默认使用 `aspect_ratio="1:1"` 和 `image_size="1K"`：

- 单个完整角色，居中、全身、轮廓清晰，适合约 160～300 px 的桌面悬浮展示。
- 背景透明；无场景、地面、边框、文字、Logo、水印、投影或脱离角色的装饰。
- 四周保留安全留白，身体和耳朵、尾巴等部位不得裁切。
- 优先通过姿态、表情和材质表现个性，避免过细、缩小后不可读的细节。
- 有照片时，把可用的工作区路径放入 `reference_images`；不得悄悄丢弃参考图改成无关角色。

生成失败时报告 Provider 返回的真实原因。参考图编辑不受当前 Provider 支持时，提示用户切换支持参考图的图片模型，不要假装已经保留照片身份。

## 应用到桌面

1. 使用 `image_gen` 原样返回的图片路径，不猜测文件名。
2. 调用 `desktop_pet action=apply`，将路径放入 `imagePath`。如果使用了用户照片，同时把原图路径放入 `sourcePath`；`image_gen` 返回了 `provider` / `model` 时也原样传入。
3. 默认启用桌宠并保持置顶；用户指定大小时将比例换算到 `scale` 的 `0.65..1.35` 范围。
4. 成功后简短报告桌宠已显示，以及实际使用的形象方向；不要重复输出内部绝对路径。

用户明确说“生成桌宠”“设为桌宠”“换成这个桌宠”即表示允许生成后立即应用，不需要二次确认。

## 管理现有桌宠

- 查看：`desktop_pet action=status`
- 显示：`desktop_pet action=show`
- 隐藏：`desktop_pet action=hide`
- 调整：`desktop_pet action=configure`，传 `scale` 和/或 `alwaysOnTop`

本 Skill 面向 Astro 当前的静态透明桌宠窗口，不生成 Codex 8×11 动画图集，也不修改应用源码。
