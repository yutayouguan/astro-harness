---
name: desktop-pet-creator
description: 从用户照片生成 Astro 桌宠、可选配套壁纸、保存和切换宠物场景。用户说“生成桌宠”“配套宠物壁纸”“宠物场景”“换一个桌面伙伴”“显示/隐藏桌宠”等请求时使用；普通图片创作不使用。
astro_bundled_rev: 8
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

### 动画与独立动作片段

- 先区分静态形象与动画。`image_gen` 产出位图，不会自动变成可播放动画；`desktop_pet action=apply` 的 `imagePath` 仍是静态图片入口。不要把一张角色图或未经处理的联系表当动画应用。
- Astro 内置奶糖无需图片模型即可使用。设置页“使用内置奶糖”可启用；用户的自定义宠物不能被静默替换。`status` 返回的 `motionClips` 可用于判断是否具备独立动作片段。
- 外部 pet/hatch 技能仅作创作参考。Astro 的独立动作片段不受每行8帧或统一动作行数限制；按动作需要设计进入、循环、退出，优先保持身份、体积、落脚点和首尾姿态连续。先修播放/定位问题，再按缺失姿态补图，不用重复帧冒充新中间帧。
- 动画包通过桌面设置中的“导入动画桌宠”选择 `pet.json`。现有 `spriteVersionNumber:2` 主图集可继续承载已验证的基础动作，新增 `motionClips` 独立覆盖 `kneading`、`grooming`，各片段有自己的素材、网格和时序；导出场景会一起打包，不丢失动作。
- `motionClips` 是按动作名索引的对象。每项包含包内相对 `path`、`frameWidth`、`frameHeight`、`columns`、逐帧 `durationsMs`、从0开始的 `loopStart`、不含末端的 `loopEnd` 和 `loopRepeats`。入口段和退出段各播一次，只重复中间循环段。当前安全界限：每片段1–128帧、每帧20–2000ms、最多8次循环、总长不超过60秒、图集不超过4096×4096。
- 先生成同一角色的连续姿态，保留全部原始素材，再去背景、统一比例和基准线、编排时序。姿态超出模型未严格遵循的隐形格线时，应按完整轮廓提取，不直接裁掉耳朵或尾巴。当前工作区存在 `tools/desktop-pet/assemble_motion.py` 时可复用；不要假设此脚本已随独立应用安装。
- 在白底、深底及实际小尺寸下检查：无阴影底板、无裁边或身体透明洞；首尾回到待机；循环不跳姿势；暂停/任务事件能打断；桌宠不能抢主窗口焦点或拦截透明区域的点击。完整素材集验证后再一起应用，不能以编译成功代替原生播放验收。
- 有图片生成授权时按已授权范围执行，不反复询问同一授权；失败时报告具体原因与保留的进度。不要修改模型配置或强行跳过用户的初始化流程来完成演示。

### 场景套装与预览

- 配套壁纸是可选的额外图片请求，用户没有选择时不生成。只作用于 Astro 应用背景，不修改系统桌面壁纸。
- 生成新宠物后优先调用 `desktop_pet action=save_scene`，传 `name`、原样的 `imagePath`，可附 `sourcePath`、`provider`、`model`。这只收藏并提供预览，不替换当前桌宠。返回 `state.scenes` 中新场景的 `id`，后续原样用作 `sceneId`。
- 用户需要配套壁纸时，再调用 `image_gen`，以刚生成的宠物图片作为 `reference_images`；横向构图、相同画风与配色，中心安静、右下留白。默认只画生活环境，不重复画宠物；用户选择肖像壁纸时才包含同一只宠物。
- 壁纸成功后调用 `desktop_pet action=save_scene`，传已有 `sceneId` 与 `wallpaperPath` 绑定，不重复创建宠物。壁纸失败时保留场景并报告真实原因；重试只生成壁纸，不重新生成宠物。
- 用户确认应用后调用 `desktop_pet action=apply_scene`，传 `sceneId` 和 `mode`：`all` 整套并开启联动；`pet` 仅桌宠并关闭联动；`wallpaper` 仅壁纸；`linked` 根据现有联动设置切换。没有壁纸的场景仅可用 `pet`。
- `desktop_pet action=status` 可查看收藏。切换已有场景不调用 `image_gen`。`configure followWallpaper=true/false` 控制联动；未绑定壁纸保留当前桌宠，关闭的桌宠不会因普通壁纸切换自动显示。
- 用户明确要求“生成并应用整套”时，可完成生成后直接 `apply_scene`；仅“生成”则先预览。

### 宠物与场景管理

- `state.scenes` 中 `pet.petPath` 相同的场景属于同一只宠物。用户说“给奶糖换一个海边的家”时，先 `status` 找到它，再 `edit_scene`，`edit={action:"duplicate",sceneId:"已有场景 id",name:"海边的家"}`，新场景复用形象，不重新生成宠物。用新场景身份图生成壁纸后再绑定。
- 改宠物名：`edit={action:"rename_pet",sceneId:"...",name:"奶糖"}`，所有同身份场景同步；改场景名使用 `rename`。
- 收藏：`edit={action:"favorite",sceneId:"...",favorite:true}`；暂停/恢复：`edit={action:"pause",paused:true/false}`。
- 移除收藏使用 `delete`，传 `sceneId` 和 `confirmActive:false`。如果返回正在使用，先说明移除只删除场景记录、当前画面和素材保留，并取得用户确认后才传 `confirmActive:true`。不通过 shell 删除素材。
- 桌面设置页可导出场景素材包；默认只导出桌宠及壁纸，不包含原始照片。原生桌宠右键菜单支持场景切换、暂停、隐藏和打开设置。

### 直接应用已有图片

1. 使用 `image_gen` 原样返回的图片路径，不猜测文件名。
2. 调用 `desktop_pet action=apply`，将路径放入 `imagePath`。如果使用了用户照片，同时把原图路径放入 `sourcePath`；`image_gen` 返回了 `provider` / `model` 时也原样传入。
3. 默认启用桌宠并保持置顶。大小的界面 100% 对应内部 `scale=0.30`（原界面75%的实际大小），界面范围为50%–200%，内部范围为 `0.15..0.60`。用户给百分比时按 `scale=百分比/100×0.30` 换算，不把界面的100%误写成 `scale=1.0`。现有宠物和场景保存的内部缩放值保持不变，不因刻度调整而重缩放。
4. 成功后简短报告桌宠已显示，以及实际使用的形象方向；不要重复输出内部绝对路径。

用户明确说“设为桌宠”“换成这个桌宠”时可直接应用，不需要二次确认。

## 管理现有桌宠

- 查看：`desktop_pet action=status`
- 显示：`desktop_pet action=show`
- 隐藏：`desktop_pet action=hide`
- 调整：`desktop_pet action=configure`，传 `scale` 和/或 `alwaysOnTop`
- 行为偏好：`configure preferences={quietMode:true, positionLocked:true}`；支持 `snapToEdge`、`hideInFullscreen`、`presentationMode` 与 `activityIntervalSecs`（15–300秒）。仅传需要修改的字段，不覆盖其它偏好。安静模式停止自动大动作；用户仍可通过右键手动播放动作。
- 位置由原生拖动结束时保存到显示器工作区域，锁定后不可误拖。设置/右键有“回到屏幕内”；不要通过 shell 重写坐标或伪造显示器信息。
- 全屏自动隐藏与手动演示隐藏不修改 `enabled`；macOS检测前台全屏窗口几何，不截屏、不读取窗口标题；其他平台支持 Astro 自身全屏与手动演示。可用托盘“恢复桌宠显示”退出演示，并临时覆盖当前全屏隐藏。
- 场景首次保存时记住当前大小、位置和行为偏好。`edit_scene edit={action:"capture_preferences",sceneId:"..."}` 更新场景偏好，要求当前正在使用同一只宠物。仅壁纸应用不改变桌宠偏好；整套/仅宠物应用恢复它们，不改变全局演示意图。导出/导入会携带 `scenePreferences`；旧场景无该字段时保留当前偏好。
- 动作片段可显式选择 `neutralBookends:true`，首尾使用主图集的同一中立帧，避免返回待机时跳形；不对第三方动作强制启用。真实多屏、全屏或播放验收未完成时必须说明，不用测试数量替代实际效果。

本 Skill 面向 Astro 桌宠和场景操作。创作格式服务于实际播放效果；只在明确需要交换兼容包时遵循外部图集规范。不要把设置页能力虚构成不存在的 `desktop_pet` 工具参数，也不通过本技能改动应用源码。
