# 奶糖局部动作来源

`grooming-local-edits.png` 是2026-09-13通过用户授权的 Azure `gpt-image-2.5-sunburst` 编辑得到的前爪和嘴部局部像素。生成使用 imagegen 技能的标准CLI，未修改该CLI；原始请求与完整图片保留在 `output/imagegen/real-pet-motion/grooming-local-edit.md`、`naitang-grooming-local.png`。

本文件不是完整宠物图，也不是图集动画成品。它保留固定4×6网格中的选定编辑区域，其余完全透明。`compose_grooming_layers.py` 将它合成回原始坐姿，只允许已校准的局部mask修改像素；头部、身体、尾巴与其他足部从原图保留。API的未选中帧不会参与运行时。

生成意图：保持相机、身体、脸与比例不变，只让屏幕左侧前爪抬起到嘴边，舌头接触爪子后收回，再将前爪放回原处；背景纯洋红以便抠图。源提示词的完整副本见同目录 `grooming-prompt.md`。

重现APNG无需调用模型：

```sh
uv run --offline --no-project --with pillow python tools/desktop-pet/compose_grooming_layers.py apps/desktop/src/assets/pets/naitang/spritesheet.webp apps/desktop/src/assets/pets/naitang/source/grooming-local-edits.png output/qa/naitang-grooming-rebuilt
```
