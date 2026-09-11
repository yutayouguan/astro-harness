# 全局控件字号回归与样式层入口

## 原因

风车面板组件曾直接导入含 `@layer features` 的样式。组件依赖图先于
`main.tsx` 下方的全局样式执行，生产 CSS 因而以 `@layer features { … }`
开头，全局层顺序声明反而出现在后面。CSS 层第一次出现后顺序就固定，
结果是基础层 `button, input, select, textarea { font: inherit }`
覆盖了功能层的字号与字重，许多控件变成默认 16px。

局部 Storybook 先载入全局样式，没有复现正式 App 的加载顺序，不能单独
作为此类回归的验证依据。

## 修复与约束

- `main.tsx` 的第一个运行时 import 必须是 `styles/index.css`，早于任何组件。
- 全局层顺序仍只在 `styles/index.css` 声明。
- 风车样式通过 `@import "./features/desktop-ambience.css" layer(features)`
  注册，不再由组件抢先创建同名层。
- 不修改控件设计字号、用户缩放设置、默认字体或材质选择。

## 回归检查

`npm run build` 在 Vite 构建后运行 `scripts/check-style-layers.mjs`，读取
`dist/index.html` 实际引用的 CSS，并检查第一个层声明是否为完整的全局顺序。
该检查已确认能拒绝修复前的错误构建。

`npm run test:fonts` 构建后测试真实生产 CSS，不加载 Storybook 或真实 Tauri
状态。Chromium/WebKit 两个引擎、明暗主题分别检查以下字号以及关键字重：

| 控件 | 预期字号 |
| --- | --- |
| 宠物管理、切换、搜索、筛选、添加 | 13px |
| 模型提供商分组标题 | 10px |
| Agent / 请求批准 | 12.48px |
| 浏览器地址栏 | 11.5px |
| 风车撤销、管理场景与壁纸 | 11px |

测试使用当前环境已安装的 Playwright Chromium/WebKit；CI 需准备这两个引擎。
构建检查与 4 项源码/解析测试、4 项生产浏览器测试通过。当前共享工作区全量
前端测试为 953/955，两项既有失败在侧栏信息架构与窗口侧栏占位测试。
