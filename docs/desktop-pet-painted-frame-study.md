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
- [x] 用户确认眨眼小样的形象和节奏（2026-09-13：“可以，继续”）。
- [ ] 确认后按相同二维流程补做其余动作；不同解剖动作使用独立遮罩与关键姿态，不复用眼睛遮罩。
- [ ] 全部动作通过视觉与原生播放验收后，统一接入现有 motionClips；本轮没有安装或原生验收。

预览 GIF 的浅色背景仅用于观看；`blink-study.apng` 与单帧 PNG 使用原图真实 alpha。

## 续作：踩奶与舔爪（2026-09-13 至 09-14）

- 沿用同一 Azure 端点、凭证和 `gpt-image-2`；没有使用 Blender、换模型或调用声音服务。
- 4 次 imagegen CLI 编辑均返回图片：踩奶24格（226.5秒）、舔爪24格（239.3秒）、抬爪补帧8格（203.3秒）、放大右爪补帧8格（98.8秒）。提示词、返回原图与色键去除图均保留在 `output/imagegen/naitang-painted-actions-20260913/`。
- 第一批不能直接发布：踩奶缺少右爪中间高度，舔爪入口骤然抬至嘴边，退出有横跨身体、被局部遮罩截断的爪子；已补绘并弃用有问题的姿态。
- 最终踩奶27帧、14张不同合成姿态；舔爪28帧、21张不同合成姿态。放回爪子使用抬起路径的逆序，明确计入复用，不冒称新增绘制帧。未用交叉淡化、光流或补重复帧凑数。
- 踩奶使用按观察到的脚尖高度排序的左爪帧和放大重绘的右爪帧。右爪补帧最后一格仍有旧落地爪残块，未使用。
- 先用不动的眼睛／耳朵区域做仅平移配准，不按变化的全身包围盒缩放；再局部合成。遮罩外 RGBA 逐字节不变，三个新动作共用完全相同的原始中立帧。
- 去掉色键处理留下的低透明度深色地面残迹；透明边缘使用预乘 alpha 混合。仍保留边界裁切检查，不以放宽检查通过素材。
- 联系表检查覆盖白、黑、棋盘背景；脚底指标变化：踩奶1px、舔爪0px。该数字不等于原生视觉自然度已验收。

### 候选包与验证边界

`output/imagegen/naitang-painted-actions-20260913/qa-package/pet.json` 与同级 `qa-package.zip` 包含完整12动作。仅替换 `idle/kneading/grooming`，其余9个动作逐字节沿用当前素材；不宣称其余动作已经重绘或通过自然度验收。

`paint_action_frames.py` 负责导图／局部合成／联系表／正常及3倍慢放；`package_painted_study.py` 检查全部APNG尺寸、alpha、帧时长、loop范围以及三个新动作的共同首尾，再打包，不安装、不打开发布开关。脚本的遮罩和帧排序只针对本次奶糖源图及原始返回图，不是通用任意宠物自动校准。

原生尝试：复用了 `Astro Pet Motion QA.app`（独立bundle ID `com.astroagent.petmotionqa`），确认 PID 使用 `/tmp/astro-config-native-pet-motion-udQv9S/home/sessions/state.db`，没有复制凭证或改正式数据。单击进入设置、桌宠、创建和文件选择器成功；导入候选返回 `Motion clip exceeds limits`，宠物库仍为原来的2只，没有安装成功。

这份验收二进制构建时间为2026-09-12 09:48:55，早于当前APNG校验源码的17:17:28；二进制中也没有当前APNG专用校验文字。旧图集的4096高度限制会拒绝27/28帧的一列序列，当前源码已对APNG排除此旧图集尺寸上限。没有通过删帧或修改发布开关绕过；需更新隔离验收构建再确认导入与播放。

- [x] 保留已获用户确认的眨眼候选。
- [x] 踩奶／舔爪绘制、补帧、裁剪、局部透明合成与候选打包。
- [x] 新工具13项定向测试通过；覆盖透明边缘、保护区、配准、错误形象、APNG时长及伪静态PNG。
- [x] 桌宠素材工具完整45项测试通过；`cargo test -p types pet_motion --lib` 的2项测试通过。
- [x] 更新与当前APNG协议一致的独立验收构建（2026-09-14，见下方续验）。
- [ ] 原生正常／慢速播放、单击、拖动、透明穿透、尺寸／焦点验收。
- [ ] 其他继承动作与新动作组合检查；整体通过后再统一应用到正式桌面。

截至本轮，正式桌宠、场景与发布门禁均未修改，候选包的 `approved/installed` 仍为 false。

收尾时 CUA 报告 Mac 已锁屏，未继续尝试操控或解锁。只结束本轮确认的独立 QA PID 23528，未停止正式应用；原生续验还需用户解锁。

### 从本轮保留图片重建（无模型调用）

```sh
python tools/desktop-pet/paint_action_frames.py compose output/imagegen/naitang-painted-actions-20260913/kneading output/imagegen/naitang-painted-actions-20260913/kneading/painted-alpha.png --paw output/imagegen/naitang-painted-actions-20260913/kneading-paw/painted-alpha.png
python tools/desktop-pet/paint_action_frames.py compose output/imagegen/naitang-painted-actions-20260913/grooming output/imagegen/naitang-painted-actions-20260913/grooming/painted-alpha.png --entry output/imagegen/naitang-painted-actions-20260913/grooming-entry/painted-alpha.png
python tools/desktop-pet/package_painted_study.py apps/desktop/src/assets/pets/naitang/apng output/imagegen/naitang-painted-blink-20260913 output/imagegen/naitang-painted-actions-20260913 output/imagegen/naitang-painted-actions-20260913/qa-package-new
```

Python 环境需要 Pillow 与 NumPy；本轮使用 Codex bundled Python。原始图片和候选包保留为本地生成物；本轮 Git 提交只包含合成／打包工具、测试和记录，不发布未验收素材。

## 新版原生验收续接（2026-09-14）

复用 `tools/verify-config-native.mjs` 的 marker-guarded 隔离构建，而非重启旧的 Motion QA 二进制。构建后脚本恢复普通开发二进制；不导入正式配置、凭证、宠物库或场景。

通用配置验收窗口被其他操作连续切换页面，因此增加明确的 `--pet` 入口：独立 `Astro Pet QA.app` / `com.astroagent.petqa.<随机后缀>`，恢复清单保留 `purpose=pet`。只允许固定 config/pet 两种类型，不能改成正式应用 ID 或任意路径；原有配置验收命令及清单仍可用。

- 构建成功：桌宠专用 debug 二进制2分38秒，普通开发构建恢复26.98秒。
- 桌宠专用清单：`/var/folders/0s/06ngl19n2rqfjmyngm2tgcgh0000gn/T/astro-config-native-5puajl/manifest.json`。
- App：同目录 `Astro Pet QA.app`，ID `com.astroagent.petqa.5puajl`；独立数据根为同目录 `home`；前端使用私有端口54297，不占用1420。
- 默认通用配置 QA 清单另存于 `astro-config-native-fs1hNj/manifest.json`，已被其他操作使用，不把它当作桌宠验收结果或擅自关闭。
- 通过 CUA 在桌宠专用 App 中单击进入偏好设置、桌宠、创建页面。点击导入按钮时 Mac 锁屏；没有继续 GUI 操作或绕过锁屏。
- 7项启动／清单恢复测试通过，包括 pet/config 隔离、拒绝伪造用途／正式 ID、根目录、符号链接和外部 URL。
- 当前 Rust APNG 校验2项测试通过，新增回归明确验证多于19帧的独立APNG不受旧图集4096高度限制。
- 完整真实候选通过当前 Rust `import_animated_pet_at` 的显式测试：全部12动作在临时目录中导入、复制后字节与时长不变、状态回读一致；`enabled=false`、没有活动宠物或桌面图片，证明导入不自动应用。

重跑真实导入器测试（只写新的临时测试目录，不操作原生UI或真实用户资料）：

```sh
ASTRO_PET_QA_MANIFEST='/Users/iswm/Desktop/04-知识库/Rust/code/astro/output/imagegen/naitang-painted-actions-20260913/qa-package/pet.json' cargo test -p astro-harness imports_external_apng_candidate_without_applying --lib -- --ignored --nocapture
```

新建验收环境用 `node tools/verify-config-native.mjs --pet`；既有启动进程退出后可用 `--resume <上述manifest.json>` 恢复，无需重编译。已有进程仍运行时直接使用该 App，不重复启动私有Vite端口。

**当前结论**：旧构建拒绝APNG的阻碍已解决；完整候选的真实导入链通过。不是原生播放验收完成。请在解锁且桌宠专用窗口空闲时继续导入、预览、应用到隔离桌面，再检查焦点、拖动、透明穿透、尺寸和组合动作。正式桌宠及 `apng-release.json` 仍未改动。

## 导出与验收可靠性优化（2026-09-14）

这轮不新增图片模型调用、不改已确认的形象或眨眼节奏；优化的是复核准确性与失败保护。

- 修复预览潜在错帧：`write_apng` 会合并相邻相同帧，旧预览却把合并后的索引用在原始帧列表。现在联系表、GIF、逐帧测量统一读取真正编码后的APNG；传入未同步的帧列表会明确拒绝。
- 记录原始帧数、编码帧数、每个编码帧对应的原素材格和输入哈希；联系表附实际帧时长，避免把原始姿态数当作播放帧数。
- GIF按累计时间取整到10ms，不再把每个42ms独立截为40ms而不断加速。APNG是精确时序来源，GIF仍是10ms精度的辅助预览。
- APNG导出先写临时文件，完整解码校验成功后才原子替换目标；失败保留旧候选并清理本次临时文件。
- 写文件前拒绝非整数／越界帧时长、过长重复动作、超过128个编码帧及合帧后超限时长。
- 新增 `compose --output <新目录>`，复核产物可另存，不覆盖已获确认的候选。

新复核目录：`output/imagegen/naitang-painted-review-20260914/`，含两个动作的白／黑／棋盘联系表、正常／慢速GIF、来源映射和完整 `qa-package/`、`qa-package.zip`。新旧包的12个APNG SHA-256全部一致：修正验收工具没有改变猫的画面、节奏或透明度。

验证：素材工具63项测试通过；新增覆盖合帧后的真实红→蓝→红预览顺序、GIF累计时长误差、非法时长与导出失败时的旧文件保护。新复核包再次通过当前Rust真实导入器，未自动应用桌宠。

已通过 `--resume` 恢复桌宠专用验收环境；CUA报告Mac仍锁屏，因此没有完成GUI导入／播放／拖动／焦点／穿透检查，发布开关继续保持关闭。解锁后应优先在已有专用窗口继续，不重复创建QA环境。
