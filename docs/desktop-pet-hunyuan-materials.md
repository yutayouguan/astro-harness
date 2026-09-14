# Hunyuan 宠物材质工作副本

2026-09-14：对用户提供的奶糖、布丁GLB进行了第一轮非破坏性材质优化。**尚未重拓扑、绑定骨骼或替换桌面宠物。**

## 结果与文件

工作目录：`output/hunyuan-materials-20260914-GQPpJm/`

- `before-materials.blend`：编辑前备份。
- `hunyuan-material-study.blend`：两只宠物的材质工作副本；原导入场景继续保留。
- `material-comparison.png`：奶糖侧背面前后对比及布丁处理后预览。
- `{naitang,pudding}-{front,three-quarter,side,back}-coat.png`：真实Blender渲染，不是模型生成的效果图。
- `source-preservation.json`：源对象、网格计数、材质绑定检查。
- `render-validation.json`：正面脸部渲染差异及透明轮廓检查。

奶糖的新场景为 `HY_Coat_naitang_GQPpJm`，布丁为 `HY_Coat_pudding_GQPpJm`。

## 材质调整

奶糖：第一版规则条纹未采用。最终使用已授权 Azure `gpt-image-2.5-sunburst` 经 imagegen 标准CLI生成的自然虎斑毛皮颜色纹理，再在Blender中局部映射。按表面朝向混合背部和侧面投影，减少侧面拉伸；独立的前向保护遮罩避免覆盖脸、眼睛和正面原有花纹。

新增 `tabby-fur-albedo.png` 和高频细节派生图 `tabby-fur-detail.png` 已打包进 `.blend`，无需依赖外部图片路径；完整提示词为 `tabby-texture-prompt.md`。它们是设计用毛皮纹理，不是经过物理测量的PBR扫描，也不是独立毛发几何。

布丁：不增加花纹、不改变颜色，仅在侧背部保守加入微表面细节和粗糙度下限。该变化较轻，不将其描述为重新制作了毛发系统。

## 保留与验证

- 源GLB的SHA256与导入前相同，原始贴图像素未写入。
- 新对象使用对象级材质覆盖，各自材质和节点树与源对象独立。
- 本轮副本仍共享只读网格；**后续做拓扑、形态键、权重等几何编辑前，必须先复制网格数据**，不能直接改共享数据。
- 奶糖原网格保持800,360顶点、1,500,000三角面；布丁保持830,880顶点、1,500,004三角面。
- 相同相机、灯光下，正面脸部8位RGB平均差异：奶糖各通道小于0.006，布丁小于0.0001。两只宠物透明轮廓的alpha完全一致。差异为渲染检查，不是“每个颜色像素完全相同”的宣称。
- 已在Blender中检查材质/节点树隔离、图片打包状态，脚本语法检查通过。

## 工具与下一步

`tools/desktop-pet/refine_hunyuan_materials.py` 通过MCP分阶段执行。命名空间需提供 `MATERIAL_OUTPUT`，可提供 `SOURCE_SCENES` 覆盖源场景映射。流程为 `create` → `apply_tabby_texture` → `fix_side_projection` → `render` → `verify_and_save`，已有目标场景拒绝覆盖。

当前材质含Blender节点投影。后续若需要导出便携GLB，应先将最终颜色/法线等烘焙到独立UV贴图；不能假设直接导出会保留这些程序节点。

下一阶段：确认侧背面观感，再准备独立动画控制网格，整理可动部位与骨骼。此工作副本没有动画，也不代表眨眼、舔爪或行走已经完成。
