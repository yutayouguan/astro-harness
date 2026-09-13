# 奶糖 APNG 候选资源

完整发布仍由上级 `apng-release.json` 审核门控制；本目录存在不等于已应用到正式桌面。

`kneading.apng` 已改为从原坐姿像素制作的局部前爪动作：49帧、32帧主体循环、42ms动态帧间隔；身体、头部、尾巴和后足不变，首尾与混合待机使用同一原图。仅左右前爪交替抬起，最大5源像素；不声称这些是图片模型生成的新姿态。

重现命令：

```sh
uv run --offline --no-project --with pillow python tools/desktop-pet/bake_naitang_kneading.py apps/desktop/src/assets/pets/naitang/spritesheet.webp output/qa/naitang-kneading-layered-20260913
```

生成后的 `clip.json` 对应两个清单中的 `kneading` 项，测试要求文件与时序精确重现。不要用旧版图集转换器 `export_apng.py` 覆盖本目录：它只负责旧素材格式转换，不包含此动作重制。

其他动作继续按交付清单逐项验收，不能因为踩奶的像素与循环测试通过而放行整包。

`grooming.apng` 使用同一坐姿与局部模型编辑合成，19帧、10帧舔爪循环，首尾精确回到原姿态。可重现源位于 `../source/grooming-local-edits.png`。它不再采用之前整只猫重新生成后缩放对齐的候选；全套正式发布审核仍未完成。
