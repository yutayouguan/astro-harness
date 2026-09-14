# Hunyuan 桌宠：转头、耳朵与尾尖组合研究

输出：`output/hunyuan-secondary-20260914-SPQ4T0/`。
本轮仍是实验素材，不替换桌面、不更改发布门禁。源 GLB 和原材质研究场景只读。

## 修复的基础问题

新复制并链接的 Blender 对象尚未完成依赖图计算时，`matrix_world` 可能仍是
单位矩阵。首轮创建骨骼过早读取该值，忽略 GLB 的 Y-up → Z-up 旋转。
小幅头部动作掩盖了问题，但尾部的错误旋转支点暴露了它。

修复：复制后先刷新目标 view layer，赋予骨骼正确矩阵，再刷新并断言骨骼与
网格矩阵完全一致。`check_hunyuan_rig_coordinates.py` 在真实 Blender 内
用新链接的旋转网格调用实际构建器，验证矩阵及运动中的刚性头部顶点。
测试只清理自身新建的临时对象，不删除预先存在的场景。

之前的头部 APNG 是旧实验，不能继续用于验收。本轮完整重新渲染。

## 本轮控制与节奏

- 保留转头骨骼，新加左右耳、尾部和尾尖骨骼，共 6 根。
- 奶糖：左右耳 2° 轻动，尾尖基段 1.2°，末段延后 2 帧响应。
- 布丁：垂耳 3° 小幅回弹，尾部基段 6°，末段 60% 幅度并延后响应。
- 耳朵先响应，接着转头、短暂停留，尾尖做递减摆动，最后回到初始姿态。
- 72 帧 / 24 fps / 3 秒，384×384 透明 RGBA；循环边界第 73 帧不重复导出。
- 维持本轮预览的 Cycles 12 samples；并非最终质量渲染或完整动作库。

权重使用原模型坐标下的专用平滑区域，不是适用于任意宠物的自动绑定器。
每个顶点的权重以 1/65536 精度整数分配，总和严格为 1；排序分组写入，
避免对每一个权重值反复扫描整个高模。完整高模/UV 不重建、不减面。

## 视觉检查与限制

先输出正/背面彩色权重图：红色尾部，绿色/蓝色左右耳，灰色为未受影响区域。
发现并修复奶糖尾尖权重碰到前腿、耳部权重碰到眉部的问题。
过大的耳/尾角度使部分局部网格明显拉长，已缩小动作；不是扩大阈值来放行。

逐帧检查 73 帧：

- 骨骼与网格坐标一致；回位帧恢复原始顶点。
- 前爪与臀部保持固定；面部核心顶点只跟随刚性头骨，不额外受耳部拉扯。
- 无顶点越过地面；原始坐标和副本绑定姿态 SHA256 不变。
- 过滤短于 1e-5 的数值敏感边后，最大边长相对变化：奶糖约 24.44%，
  布丁约 24.04%。这是局部变形上限检查，不等于完整碰撞检测或审美验收。
- 打包检查透明轮廓、无裁切、首尾、3 秒总时长及所有解码帧；相同停留帧允许合并。

模型仍是身体、四肢、眼睑合并的高模。大幅摇尾、自然眨眼、踩奶、舔爪、行走
需要后续解剖分区/拓扑及控制器工作；当前结果不能标为全部完成。

## 复现与测试

Blender 中追加 `HY_Coat_naitang_GQPpJm` 和 `HY_Coat_pudding_GQPpJm`，先保存备份。
执行 `rig_hunyuan_secondary_study.py`，传入独立 `SECONDARY_OUTPUT` 目录，
依次 `create(pet)`、`render_weights(pet)`、`validate(pet)`、`render(pet, frame)`、`save()`。
脚本调用改正后的头部构建器，不依赖临时 driver namespace。

```sh
uv run --no-project --with pillow --with numpy python -m unittest discover \
  -s tools/desktop-pet -p 'test_*hunyuan*.py' -v
uv run --no-project --with pillow python tools/desktop-pet/package_hunyuan_head_study.py \
  output/hunyuan-secondary-20260914-SPQ4T0 --study secondary-study
```

Blender 回归测试单独执行 `check_hunyuan_rig_coordinates.py`。
