# 创建 Agent：头像 / Emoji 右侧独立抽屉

日期：2026-07-15  
状态：待用户审阅

## 背景

创建 Agent 引导卡里，头像预设（Cover 插画）目前内嵌大网格，Emoji（Lucide）用居中弹层。内容又高又挤，且「封面 / Avatar / Emoji」概念容易混。

已确认（对话）：

- 两个**独立**右侧抽屉（非 Tab 合一、非复用聊天侧栏）
- 头像抽屉内含：**预设插画 + 上传自定义**；卡片上只留预览/入口
- 实现路径：**方案 A**（独立抽屉组件 + Lucide 改抽屉壳）

## 目标

1. 创建卡「外观」区变短：两行入口（头像、Emoji），不再内嵌插画网格。
2. 点入口从右侧滑出抽屉选择；选中或上传成功后关闭并回填预览。
3. 视觉与交互对齐现有 `mcp-add-drawer` / `skills-preview` / `cron-run-drawer`。
4. 存储语义不变：头像 → `assets/avatar.*`；Emoji → `assets/emoji.*`；展示仍优先 avatar。

## 非目标

- 不改后端 `set_pending_agent_icon` / IDENTITY 字段协议。
- 不把选择器塞进 `ChatRightPanel`。
- 不引入「用 Lucide 填头像槽」第三条路径。
- 本轮不抽全局通用 `SideDrawer` 抽象（允许少量 CSS 对齐复制）。

## 信息架构

### 创建卡（`AgentCreateGuide`）

| 行 | 内容 | 动作 |
|---|---|---|
| 头像 | 缩略图 + 标题/短说明 | 「选择/更换」→ 开头像抽屉；有值可「清除」 |
| Emoji 小图标 | 同上 | 「选择/更换」→ 开 Emoji 抽屉；有值可「清除」 |

卡片内移除：`CoverPicker` 网格、「或上传自定义」内联大槽。

### 抽屉 A：头像（新建 `AvatarPickerDrawer`）

- 遮罩 + 右侧面板；Esc / 点遮罩 / 关闭按钮关掉。
- 顶栏：标题（如「选择头像」）+ 副文案（写入 `assets/avatar`）。
- 主体可滚：内置插画网格（复用 `CoverPicker` / `AGENT_COVERS`）。
- 分区或底栏：「上传自定义」；说明会覆盖预设。
- 选中预设或上传成功 → `onChange` / 现有 `pickCover`·`pick("avatar")` → **关闭抽屉**。

### 抽屉 B：Emoji（改造 `LucideIconPicker`）

- 壳从居中 modal 改为右侧抽屉（同遮罩/滑入规范）。
- 保留搜索、颜色、渐变、描边/填充、网格点选。
- 保留「上传图片」入口（覆盖 Lucide，仍写 emoji 槽）。
- 点选 Lucide 或上传成功 → 现有 `onSelect` / `pick("emoji")` → **关闭抽屉**。

## 组件与文件

| 项 | 说明 |
|---|---|
| `AgentCreateGuide.tsx` | 两行入口；`avatarOpen` / `lucideOpen` 状态；去掉内嵌 Cover 网格 |
| `AvatarPickerDrawer.tsx`（新） | 头像抽屉壳 + CoverPicker + 上传 |
| `LucideIconPicker.tsx` | 布局改为 side drawer；对外 props 尽量不变 |
| `chat.css` / 可小量共用 drawer class | 对齐 mcp/skills 右侧抽屉尺寸与动画 |
| `messages.ts` | 抽屉标题、入口按钮、上传区文案（zh/en） |

逻辑（pending icon、自动建议 Lucide、cover→avatar）保持在 `AgentCreateGuide`，抽屉只负责 UI。

## 交互细节

- 同时只开一个抽屉：开 A 时若 B 开着则先关 B（反之亦然）。
- `busy`（上传/渲染中）时禁用关闭可选；至少禁用重复提交。
- 焦点：打开时 trap 到抽屉；关闭后焦点回到触发按钮（尽力而为）。
- 移动窄屏：抽屉仍全高贴右，宽度 `min(100%, ~380–420px)`，与现有侧抽屉一致。

## 验收

- [ ] 创建卡无内嵌封面网格；高度明显下降。
- [ ] 点头像入口 → 右侧抽屉含插画 + 上传；选/传后预览更新且抽屉关闭。
- [ ] 点 Emoji 入口 → 右侧抽屉为 Lucide 选择器；选/传后预览更新且抽屉关闭。
- [ ] Esc / 遮罩可关；两抽屉互斥。
- [ ] 创建后仍写入 `assets/avatar.*` 与 `assets/emoji.*`（行为与改前一致）。
- [ ] 亮/暗色主题抽屉可读，动画不挡操作。

## 风险

- Lucide 面板内容较长：抽屉内必须可滚，顶栏/搜索区 sticky 或固定，避免「选不了颜色」。
- 与聊天 `ChatRightPanel` 同开时的 z-index：抽屉应高于创建卡与聊天壳（参考 mcp-add ~80+）。
