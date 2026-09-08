---
name: desktop-pet-creator
description: 从用户照片生成 Astro 桌宠、可选配套壁纸、保存和切换宠物场景。用户说“生成桌宠”“配套宠物壁纸”“宠物场景”“换一个桌面伙伴”“显示/隐藏桌宠”等请求时使用；普通图片创作不使用。
astro_bundled_rev: 3
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

### 场景套装与预览

- 配套壁纸是可选的额外图片请求，用户没有选择时不生成。只作用于 Astro 应用背景，不修改系统桌面壁纸。
- 生成新宠物后优先调用 `desktop_pet action=save_scene`，传 `name`、原样的 `imagePath`，可附 `sourcePath`、`provider`、`model`。这只收藏并提供预览，不替换当前桌宠。返回 `state.scenes` 中新场景的 `id`，后续原样用作 `sceneId`。
- 用户需要配套壁纸时，再调用 `image_gen`，以刚生成的宠物图片作为 `reference_images`；横向构图、相同画风与配色，中心安静、右下留白。默认只画生活环境，不重复画宠物；用户选择肖像壁纸时才包含同一只宠物。
- 壁纸成功后调用 `desktop_pet action=save_scene`，传已有 `sceneId` 与 `wallpaperPath` 绑定，不重复创建宠物。壁纸失败时保留场景并报告真实原因；重试只生成壁纸，不重新生成宠物。
- 用户确认应用后调用 `desktop_pet action=apply_scene`，传 `sceneId` 和 `mode`：`all` 整套并开启联动；`pet` 仅桌宠并关闭联动；`wallpaper` 仅壁纸；`linked` 根据现有联动设置切换。没有壁纸的场景仅可用 `pet`。
- `desktop_pet action=status` 可查看收藏。切换已有场景不调用 `image_gen`。`configure followWallpaper=true/false` 控制联动；未绑定壁纸保留当前桌宠，关闭的桌宠不会因普通壁纸切换自动显示。
- 用户明确要求“生成并应用整套”时，可完成生成后直接 `apply_scene`；仅“生成”则先预览。

### 直接应用已有图片

1. 使用 `image_gen` 原样返回的图片路径，不猜测文件名。
2. 调用 `desktop_pet action=apply`，将路径放入 `imagePath`。如果使用了用户照片，同时把原图路径放入 `sourcePath`；`image_gen` 返回了 `provider` / `model` 时也原样传入。
3. 默认启用桌宠并保持置顶；用户指定大小时将比例换算到 `scale` 的 `0.65..1.35` 范围。
4. 成功后简短报告桌宠已显示，以及实际使用的形象方向；不要重复输出内部绝对路径。

用户明确说“设为桌宠”“换成这个桌宠”时可直接应用，不需要二次确认。

## 管理现有桌宠

- 查看：`desktop_pet action=status`
- 显示：`desktop_pet action=show`
- 隐藏：`desktop_pet action=hide`
- 调整：`desktop_pet action=configure`，传 `scale` 和/或 `alwaysOnTop`

本 Skill 面向 Astro 当前的静态透明桌宠窗口，不生成 Codex 8×11 动画图集，也不修改应用源码。
