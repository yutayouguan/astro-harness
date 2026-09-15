# 奶糖 Blender 外观样稿

本轮按用户确认改用 Blender 制作3D静态外观，不继续发布被否定的平面行走候选。**这是外观确认稿，尚未绑定动画骨骼，也未替换正式桌宠。**

## 交付位置

- 场景：`Astro_Naitang_Studio_Static_v2`
- 可编辑文件：`output/blender-naitang-studio-20260913/naitang-studio-v2.blend`
- 透明预览：同目录 `naitang-v2-front.png`、`naitang-v2-three-quarter.png`、`naitang-v2-side.png`
- 建模前备份：同目录 `before-modeling.blend`
- 可复现脚本：`tools/desktop-pet/blender_naitang_studio.py`（尚未随仓库提交，目前只存在于本地 Blender 会话）

现有 `Scene`、早先的空候选场景与第一版外观稿均保留。未启用 Hyper3D/Hunyuan3D，没有调用额外付费3D服务，也没有下载第三方模型。几何、毛色、眼睛和灯光均为本地Blender构建。

## 模型内容

- 头、身体、脚掌、外耳与尾巴经体素重构形成连续表面；这是静态雕塑拓扑，后续仍需为关节变形整理拓扑和权重。
- 橘白虎斑顶点色、白袜与胸口色块、凹入眼窝和曲面瞳孔。
- 约五千条短绒毛曲线，正交相机、三灯柔光、透明背景Cycles渲染。
- 眼部反光由场景灯光生成，未叠加照片或生成图片冒充3D效果。

## 分阶段复现

在Blender中先保存当前文件；用MCP执行时为命名空间传入脚本的绝对 `__file__`。新场景名已存在时脚本会拒绝覆盖。首次建模依次运行：

下面这段复现流程依赖上面那个脚本；它还没有进入仓库，运行前需要先把脚本放到 `tools/desktop-pet/blender_naitang_studio.py`。

```python
from pathlib import Path
script = Path('/absolute/path/to/astro/tools/desktop-pet/blender_naitang_studio.py')
ns = {'__file__': str(script), '__name__': 'naitang_studio'}
exec(compile(script.read_text(), str(script), 'exec'), ns)
# 每个阶段单独执行，检查输出后再继续。
ns['setup']()
ns['body']()
ns['sockets']()
ns['paint']()
ns['face']()
ns['velvet']()
ns['studio']()
ns['refine_expression']()
ns['render']('front')
ns['render']('three-quarter')
ns['render']('side')
ns['set_view']('three-quarter')
ns['save']()
```

使用节点类型而非默认英文节点名，兼容中文Blender界面；修改几何前会切换到专属场景，后续精修也按专属对象名操作。

下一步是用户确认外观，再做关节拓扑、骨骼和动作。三张静态预览不能视为行走、踩奶、舔爪或桌面交互已经验收。
