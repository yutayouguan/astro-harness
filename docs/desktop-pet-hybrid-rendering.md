# 桌宠混合渲染

2026-09-13 用户确认：不再把全部行为强制烘焙成 APNG；待机使用可控分层，完整动作用 APNG。本实现只借鉴分层/状态分流思路，没有复制 clawd-on-desk 的代码或素材。

## 当前实现

- `DesktopPetCanvas` 将 idle/look 交给分层播放器；踩奶、舔爪、其他完整动作和原生漫游时间线仍交给原播放器。已有v2图集与v3 APNG的坐姿都可识别。
- 奶糖/布丁复用原始透明像素，离线提取身体、头部/眼睑、尾巴PNG。头和眼区不同速率跟随，尾巴间歇摆动，耳区为微小纹理形变；不是重新生成整只宠物，也不是全身缩放。
- 所有校准坐标以192×208原图为准；256×208 APNG仅加左右透明边。脚掌不参与头部/眼区变换，最终canvas继续用于像素命中。
- 在浏览器中计算原图完全不透明像素的SHA256后才启用对应rig，半透明毛边忽略预乘舍入误差。不按宠物名字套用；不匹配的用户素材回到原播放器。
- 自定义照片目前不会自动产生分层骨架，需要独立制作与校准。完整动作的自然度仍需单独审核，PNG分层不等于已解决四足步态。
- PNG均为受控本地静态资源，不加载第三方SVG脚本，也没有新增图片模型调用。

## 时序与资源

- 分层素材两只合计约340KiB。已解码rig只保留两份，来源探测缓存上限24项；APNG首帧检测复用已有租约缓存。
- 注视使用与帧率无关的指数缓动；暂停保留当前相位，隐藏/离屏停止时钟，系统“减少动态”显示原始静态姿态。
- 局部动作期间约30fps，静止阶段睡眠到下一次眨眼/耳尾动作，鼠标变化可提前唤醒。切入新canvas前同步绘制首帧，避免空白闪烁。
- 仍沿用Tauri不可聚焦桌宠窗口和合成像素命中，没有复制Electron双窗口实现。此前真实QA中宠物开启时Tab单击和输入框键盘输入通过；新增分层的完整原生焦点/拖动验收因电脑再次锁屏尚未完成。

## 复现

```sh
uv run --offline --no-project --with pillow python tools/desktop-pet/build_idle_rig.py apps/desktop/src/assets/pets/naitang apps/desktop/src/assets/pets/naitang/idle-rig
uv run --offline --no-project --with pillow python tools/desktop-pet/build_idle_rig.py apps/desktop/src/assets/pets/pudding apps/desktop/src/assets/pets/pudding/idle-rig
```

单元测试：`petIdleRigMotion.test.ts`、`test_build_idle_rig.py`。浏览器测试：`playwright.apng.config.ts` 中的混合播放器与原APNG播放器用例；覆盖原图识别、不匹配回退、固定脚掌、透明角落、暂停续播、系统减少动态、APNG接管与旧版图集。

Storybook：`Desktop/PetHybrid`。白/黑背景截图位于 `output/qa/hybrid-idle-{naitang,pudding}-{chromium,webkit}.png`。

正式APNG整包发布仍受 `apng-release.json` 审核门控制。不能因为混合待机已实现而将未通过的步态标为可发布。

## 后续修复

- 混合播放器与APNG/旧图集播放器之间新增按原图来源隔离的帧保留：解码下一动作时继续显示上一帧，不以空canvas过渡；跨宠物不复用。保留帧不会发出原生漫游ready确认，仍要等新动作真实首帧绘制。
- 延迟APNG请求600ms的回归测试连续采样15个浏览器帧，没有空白帧。Chromium/WebKit共6项播放器用例通过。
- 奶糖踩奶改为校准原图的局部前爪形变，49帧、32帧主体循环。脸、身体和尾巴保持固定，首尾与待机一致；白/黑底关键帧已检查，正式全套应用仍未放行。
- 舔爪改为模型局部编辑加固定原图合成，19帧、10帧主体循环；身体不再按每帧包围盒缩放。已保留约188KiB局部源图、提示词和精确复现测试。此处新增一次已授权图片模型请求，不影响前述分层待机素材的离线来源。
