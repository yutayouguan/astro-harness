# Hunyuan 桌宠：第一轮头部骨骼验证

> 后续检查发现首轮骨骼没有正确继承新复制网格的坐标旋转。原目录
> `output/hunyuan-rig-20260914-hMipeb/` 保留作历史实验，不能作为发布候选。
> 创建流程已加入依赖图刷新与坐标一致性断言，另有真实 Blender 回归测试。
> 修正后的组合动作见 `desktop-pet-hunyuan-secondary-study.md`。

这是离线 Blender 变形研究，不是正式桌宠发布包。没有替换桌面默认宠物，
没有更改 APNG 发布门禁，也不包含行走、眨眼、踩奶、舔爪或摇尾巴。

## 输入与隔离

- 输入：已保存的 `HY_Coat_naitang_GQPpJm` / `HY_Coat_pudding_GQPpJm` 材质研究场景。
- 创建新场景，独立复制网格、材质图、相机、灯光和世界；图片像素只读共享。
- 保留 glTF 局部 Y 向上坐标，不对原网格应用变换或减面。
- 奶糖 800,360 顶点，布丁 830,880 顶点。新副本才写入顶点组与 Armature modifier。
- 当前 Blender 状态先另存 `before-rig.blend`，然后追加材质场景。

## 本轮动作

两根骨骼：固定身体的 `body_anchor` 和 `head`。局部高度 0.38–0.58
使用 quintic smoothstep 过渡权重；上部头颅刚性跟随，下半身固定。
启用保持体积。这只是小角度验证权重，不能用于大幅扭头或完整四足运动。

24 fps、72 帧、3 秒：停留 → 7° 转头并轻微歪头 3° → 短暂停留 → 返回。
Bezier 自动钳制手柄防止过冲；第 73 帧只用于循环边界，不重复输出。
Cycles 12 samples、降噪、384×384、透明 RGBA，无地面或阴影平面。
这个预览分辨率和采样数不是最终高质量素材标准。

## 工具与验证

1. 在 Blender 中追加上述材质场景，设置 `RIG_OUTPUT` 后执行
   `tools/desktop-pet/rig_hunyuan_head_study.py`，分别调用 `create(pet)`。
2. `validate(pet)` 检查所有 73 帧：脚掌顶点不漂移，回到中立姿态时
   所有顶点复位；原始坐标 SHA256、原始无骨骼状态、副本绑定姿态不变。
3. `render(pet, frame)` 输出 1–72 帧，`save()` 保存独立 `.blend`。
4. `package_hunyuan_head_study.py <目录>` 校验原生 RGBA、完整帧序列、
   非空且无裁切轮廓、循环首尾一致、确有轮廓运动，再打包两只宠物。
5. APNG 无损解码逐帧核对、总时长 3000 ms；相同停留帧可合并，
   合并后帧数不等于渲染帧数不代表丢失动作。已存在的包拒绝覆盖。

循环端点处理：实测固定 seed 下，Cycles 并行计算使首尾极少数 RGB 像素
出现 1/255 舍入差异（每通道平均差异不超过 0.000014/255），Alpha 完全相同。
打包仅允许最大 RGB 差 1、平均差 ≤0.001、Alpha 差 0 的数值噪声，并将
最后帧规范为第一帧。可见变化或轮廓偏移仍拒绝；原始 PNG 不修改。

测试：

```sh
uv run --offline --no-project --with pillow python -m unittest discover \
  -s tools/desktop-pet -p test_package_hunyuan_head_study.py -v
```

本次产物目录：`output/hunyuan-rig-20260914-hMipeb/`。
大型源模型、Blender 文件、渲染输出不加入 Git；可复现脚本和说明单独提交。

## 后续门槛

- 尾巴需要从臀部/脚掌正确划分控制区域，先验证权重图，不能只按高度拉动整片网格。
- 眼睛/眼睑/嘴部仍是合并高模的一部分；尚未建立可用于自然眨眼、舔爪的结构。
- 行走需要四肢控制、落脚约束与更合适的变形拓扑，不复用本轮头部权重。
- 完整动作与过渡通过视觉验收后，才能进入桌宠包和原生窗口验收。
