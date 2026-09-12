# Soft 乳白磨砂材质 QA — 2026-09-12

final result: passed

本结论仅覆盖本次材质变更的 Storybook 渲染和交互，不等于原生 Tauri WebView 或全部功能页验收。用户要求迁移卡片、按钮、输入框和全局背景的材质，不是复刻华为页面的布局、品牌、字体大小或照片。

## 视觉依据

- Source visual truth: `/var/folders/0s/06ngl19n2rqfjmyngm2tgcgh0000gn/T/codex-clipboard-7d430bd5-abe1-4147-b2d6-6d8986032523.png`，1320 × 1275 px。
- Implementation: `http://127.0.0.1:6006/iframe.html?id=design-soft-material--playground&viewMode=story`。
- 最终全图：`output/milky-material-20260912/final-light-wallpaper.png`，1320 × 1275 px；CSS viewport 1320 × 1275，浏览器 DPR 2，截图 API 已归一化为 CSS 像素，无额外拉伸。
- 局部：`output/milky-material-20260912/card-detail.png`（510 × 320）、`input-detail.png`（560 × 135），同一截图状态。
- 附加状态：`dark-wallpaper-max.png`、`reduced-transparency.png`、`narrow.png`，均位于上述输出目录；窄窗口 CSS viewport 720 × 900。
- Source 与最终全图在同一次视觉比较输入中打开；按材质层次比较，不把桌面/手机信息密度差异判为缺陷。

## 迭代与修复

1. 初始状态：实灰底遮住氛围色，90% 卡片填充导致透色弱。改为乳白配方，默认 79% / 32px，大面板保留背景色；控件 94%、输入 88%，沿用细边和低阴影。氛围色只叠静态薄罩，壁纸复用既有 shade 层模糊。
2. [P2，已修复] 1320px 双列样板的设置面板过窄，壁纸操作被裁切。仅调整 Storybook 列宽及单列断点，最终全图中按钮完整可见，720px 无横向页面溢出。生产布局未改。
3. [P2，已修复] 减少透明度时，通用 Glass 无障碍别名使设置卡片重新变成半透明紫底。Soft 重绑实心别名；复查设置卡片、输入和按钮均为 `rgb(250, 249, 246)`，背景模糊为 `none`，证据见 `reduced-transparency.png`。
4. 样板原有 IPC 通配空数组不是压缩设置 DTO，隐藏卡片会产生 NaN 属性告警。本样板对未展示的压缩设置返回 null；重新加载后的控制台错误/警告为空。不修改生产压缩逻辑。

## 五项视觉检查

- 字体/排版：保留 Astro 系统字体、字号和层级，正文及次要文字清晰；未复制参考中的手机大字。
- 间距/布局：生产控件尺寸、圆角、结构不变；参考的轻边缘与弱阴影迁入材质层。样板桌面/窄屏可滚动且无水平溢出。
- 色彩/材质：卡片透出背景色，乳白按钮比卡片更厚，输入框不再呈凹槽；原有强调色、危险状态、选中态保留。暗色有独立石墨配方。
- 图像：生产不新增/替换图片；样板复用现有 `desktop-pet-concept.png` 验证壁纸采样，模糊由 CSS 材质产生。参考照片、头像和品牌不属于交付范围。
- 文案：只更新 Soft 材质说明和样板说明；保留并行改动中的简短强度文案。应用内容无参考图的品牌文案泄漏。

## 验证

- 34 项 Soft 回归通过，覆盖强度每一步单调变化、0/50/100 边界、偏好隔离、最差黑白底 4.5:1 文字对比度及无障碍别名。
- 输入可编辑；Tab/Shift+Tab 焦点归聊天外壳，内层 textarea 无第二条 outline。
- 滑块键盘 Home/End/PageUp 验证 0/50/100；最大值为 72% / 64px，零强度无模糊。
- 亮暗切换、示例壁纸切换、刷新恢复通过。高对比时卡片 1px 边框且无模糊；减少透明度时实心。
- TypeScript、限定文件 Stylelint、生产构建和 CSS layer 检查通过。
- 全量测试的共享工作区快照为 1013 通过 / 4 失败：`appearanceComposition`、`desktopPetIntegration`、`sidebarInformationArchitecture`、`windowChromeSafeArea`。断言涉及其他正在修改的外观页面、透明窗口和侧栏结构，不在本次材质改动中修复；不宣称全量测试通过。

## 剩余边界

未启动或重启真实 Astro Tauri 窗口，未验证原生 WebKit 合成和全部专业功能页。图中透色取决于用户的实际壁纸/氛围配色，不能保证不同背景具有相同的桃色与蓝色分布。下一步在真实 APP 中选择 Soft 并检查常用页面。
