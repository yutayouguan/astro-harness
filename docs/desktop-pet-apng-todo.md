# 桌宠 APNG 迁移

用户要求：全部内置动作改为APNG；有问题的动画重新生成。转换格式不等于动作修复，所有素材准备并通过检查后一起应用。

- [x] 奶糖/布丁所有动作输出独立APNG候选包（12+14个文件）；保留源图集用于可重复转换，不作为v3包运行素材。
- [x] 可控逐帧APNG播放器：重播、16方向注视、透明Canvas、减少动态、加载失败；双引擎解码验证通过。
- [x] v3宠物包校验与导入导出，原生往返测试保留动作字节与默认偏好；导入不应用、不覆盖已有宠物。
- [ ] 完成素材质量验收后，接入内置安装与已有场景原子升级；保留用户自定义素材及偏好。
- [ ] 奶糖踩奶/舔爪入口重做，检查其它动作；透明、体积、锚点及原速/慢放验收。
- [ ] 运行定向测试、构建、原生验收；全部通过后应用，不把候选素材当完成品。

2026-09-12：开始迁移。沿用已授权 Azure imagegen CLI，请求重做踩奶入口；新格式代码与素材质量分开推进。既有并行眨眼修改保留，未覆盖或提交他人工作。

## 第一阶段结果（2026-09-12）

- APNG候选包：`apps/desktop/src/assets/pets/{naitang,pudding}/apng/pet.json`。每个动作一个真正的APNG，使用完整RGBA透明度、逐帧时长；脚部坐标和中立首尾在转换时按旧运行时语义烘焙。`look.apng`为可定位的16方向姿态库，不作为随机闲暇动作播放。
- 可重复导出：`python3 tools/desktop-pet/export_apng.py <原素材目录> <APNG目录>`，依赖Pillow。编码后逐帧解码比对RGBA字节与时长；只合并相邻完全相同帧，不插入伪过渡。两只宠物合计约16MB，旧源素材保留。
- v3继续使用现有清单容器，`spriteVersionNumber=3`、`spritesheetPath=idle动作路径`，`motionClips`为全部APNG动作；`columns=1`、`neutralBookends=false`，入口/重复/出口已经烘焙。v2导入与存量宠物继续可用，不直接改写用户历史文件。
- APNG按需解码、最多缓存3个动作。相同宠物切动作时保留上一姿态直到新动作就绪，避免空白闪烁；切换宠物时清空旧形象，迟到解码不会覆盖新宠物。Canvas同时用于显示和既有alpha命中测试，不让HTML图片的自主动画时钟与命中帧分离。
- 12项定向前端测试、2项APNG原生测试（含两种宠物导入/导出往返）、2项Python导出测试、2项Chromium/WebKit流程通过。另有2项Rust动作模型与17项场景回归通过。浏览器流程覆盖26个动作解码、透明度、播放变化及减少动态后稳定，不能替代原生桌面穿透/拖动验收。
- 宽范围检查曾被并行`PetTasks.stories.tsx`类型错误阻挡，后续完整类型检查通过；旧`desktopPetIntegration`的主窗口颜色方案源码断言另有1项基线失败（源码新增`isPetTaskWindow`分支，本次未修改该分支）。
- 收尾：生产前端构建（TypeScript、Vite、CSS层级检查）通过；隔离暂存快照的TypeScript检查通过，不依赖并行眨眼/任务弹窗改动。旧v2布丁导出导入回归另1项通过；未重建/重启正式原生应用。
- 图片重做：已授权Azure CLI的`gpt-image-2`两张参考图编辑仍返回`APIConnectionError / Server disconnected without sending a response`；未返回新素材，已停止后续图片请求。只读HEAD连通检查返回HTTP200，不足以证明图片编辑端点可用，更不能据此认定是额度或密钥错误。
- 质量门槛未过：奶糖踩奶/舔爪入口仍需重做；候选idle为原图集转换，不冒充已合入并行的眼部眨眼精修。其它动作需完整人工播放审查。未自动升级内置库、未切换正式桌面、未删除原素材；本轮是可验证的格式/播放器基础，不是“全部动画已修复”。

## 复现测试

```bash
cd apps/desktop
node --test src/lib/ui/petApng.test.ts src/lib/ui/petActionCatalog.test.ts src/lib/ui/desktopPetLeisure.test.ts src/lib/ui/petMotionClip.test.ts
npx playwright test --config=playwright.apng.config.ts
# 仓库根目录
cargo test -p astro-agent apng --lib
python3 -m unittest discover -s tools/desktop-pet -p test_export_apng.py
```

生成尝试的提示词沿用 `output/imagegen/naitang-entry-20260912/kneading-entry.md`：6个分离全身姿态，首尾为当前中立/目标参考，中间4姿态只逐渐转正头部、降低下巴和放松眼睑，身体、脚、尾巴锚点不动，纯品红背景便于抠图。使用imagegen CLI，无输出图片可交付。
