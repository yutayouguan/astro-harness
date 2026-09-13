# 奶糖：二维绘制帧小样

2026-09-13：用户否定 Blender 卡通候选，改为二维绘制图片帧后裁剪合成。
Blender 候选保留但不安装，不替换当前奶糖；本轮也不修改运行时或当前桌宠选择。

## 方法与来源

- 原图：`output/imagegen/real-pet-motion/naitang-reference.png`，192×208 原版奶糖。
- 新素材：已授权 Azure Image API，内置 imagegen CLI `edit`，`gpt-image-2`、high、1536×1024 PNG。一轮 CLI 调用成功，约150.5秒；不更换凭证或模型。
- 这是图片模型绘制的局部帧，不冒充 Photoshop/Krita 人工手绘。
- 输出：`output/imagegen/naitang-painted-blink-20260913/`。保留提示词、原始返回图、遮罩、原图哈希及全部合成帧。
- 采用固定4×2网格，仅接受眼睛附近 RGB；身体、脚底基准、透明轮廓保持原图，不对每帧单独裁边缩放，不交叉淡化整只宠物。
- 第7格（从0起算）提前睁眼且虹膜与原图不同，弃用。接受0–6格并以原始睁眼帧收尾；不把重复帧当成新增绘制帧。

## 复现

使用带 Pillow 的 Python 运行：

```sh
python tools/desktop-pet/paint_blink_sample.py prepare output/imagegen/real-pet-motion/naitang-reference.png output/imagegen/new-blink-study
# 用保留的 prompt.md 对 edit-grid.png / edit-mask.png 做局部绘制，得到 painted-grid.png。
python tools/desktop-pet/paint_blink_sample.py compose output/imagegen/naitang-painted-blink-20260913 output/imagegen/naitang-painted-blink-20260913/painted-grid.png
python -m unittest discover -s tools/desktop-pet -p test_paint_blink_sample.py
```

`prepare` 拒绝覆盖已有目录；`compose` 只重建传入小样目录的输出，不安装宠物。
合成工具的眼睛坐标只适用于本小样的原版奶糖，不能直接套给布丁或其他角色。

## 验证 / TODO

- [x] 原始二维形象、局部遮罩和固定网格。
- [x] 白／黑／棋盘静态逐帧检查，没有新增地面阴影或背景；保留原图边缘质量，不声称重新修复了原图的所有边缘。
- [x] 全帧 alpha 与原图一致，首尾原图一致；测试保证遮罩外 RGBA 不受生成内容影响。
- [x] APNG 解码回验、时长验证；正常／3倍慢速 GIF 预览。
- [x] 3项局部工具测试通过。
- [ ] 用户确认眨眼小样的形象和节奏。
- [ ] 确认后按相同二维流程补做其余动作；不同解剖动作使用独立遮罩与关键姿态，不复用眼睛遮罩。
- [ ] 全部动作通过视觉与原生播放验收后，统一接入现有 motionClips；本轮没有安装或原生验收。

预览 GIF 的浅色背景仅用于观看；`blink-study.apng` 与单帧 PNG 使用原图真实 alpha。
