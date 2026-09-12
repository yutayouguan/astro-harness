# 奶糖动作审查 · 2026-09-12

| Before | After（待实施） | Why |
| --- | --- | --- |
| 踩奶中立帧0→动作帧1，头部从倾斜上望突然转正，眼睑同时变化 | 补真正的头部/眼睑中间姿态，保持身体、脚底和尾巴锚点 | 脚底稳定并不能消除关节姿态跳变；增加重复帧只会延长停顿 |
| 舔爪中立帧0→动作帧1，头部朝向与低头幅度突然改变 | 补与现有两端一致的入口，连同出口和循环一起播放验收 | 必须以连续播放观感确认；不使用淡化重影掩盖缺失姿态 |

## Verdict

**Feel-breaking regressions：Block / 暂不通过。** 当前可见的入口跳变仍影响自然度，本轮没有替换素材，也不把“首尾相同”当作完整动作通过。

**Origin, physicality & cohesion：** 白底/深底联系表的逐帧检查与测量显示，踩奶和舔爪的脚底基准变化均为0px，首尾均使用主图集同一中立帧。入口轮廓变化像素数分别为2663和3570，仅作为优先检查线索，不是艺术质量阈值或人体关键点测量。

本审查针对角色动作片段，不把菜单/按钮的通用300ms时长规则套用到多秒的踩奶/舔爪行为。完整原速、慢放、动作打断、减少动态效果和原生窗口性能验收仍待继续。

## 来源与复现

- 动作清单：`apps/desktop/src/assets/pets/naitang/motion-clips.json:2`（踩奶）与`:7`（舔爪）。
- 生效帧替换：`apps/desktop/src/lib/ui/petMotionClip.ts:14`；播放序列：同文件`:27`。
- 只读审查工具：`tools/desktop-pet/audit_motion.py`。按 `neutralBookends` 替换首尾，而不是误审图集中的近似中立帧；按真实重复序列检查循环接缝。
- 工具使用 alpha>32 的轮廓，记录边界、脚底、足部区域中心和相邻帧轮廓变化；半透明毛发外缘不作为脚底。

在安装 Pillow 的 Python 环境中，从仓库根运行：

```bash
python3 -m unittest discover -s tools/desktop-pet -p test_audit_motion.py
python3 tools/desktop-pet/audit_motion.py apps/desktop/src/assets/pets/naitang output/qa/pet-motion-20260912
```

本地输出（不作为应用资源打包）：

- `output/qa/pet-motion-20260912/kneading-contact.png`、`grooming-contact.png`：白/深底联系表，已逐帧查看。
- 同目录 `kneading-slow.gif`、`grooming-slow.gif`：3倍慢放，已生成，尚未完成连续播放验收。
- 同目录 `measurements.json`：逐帧度量与真实播放序列。

## 补帧尝试与后续边界

沿用用户此前授权，仅尝试生成踩奶入口的6个姿态，以当前中立帧及目标动作帧为参考，没有重做角色或整套动作。Azure `gpt-image-2` 图片编辑请求在返回响应前失败：`APIConnectionError / Server disconnected without sending a response`。未收到任何新图片，SDK可能内部重试，无法确认计费。

请求记录在 `output/imagegen/naitang-entry-20260912/PROGRESS.md`。已停止后续图片请求；需端点/网络恢复后继续。踩奶、舔爪两条入口均完成抠图、统一尺度、原速/慢放和白深背景验收后才能一起替换，不将半成品应用到用户桌面。
