# 插画体系设计（空状态 + Agent 封面）

日期：2026-07-14  
状态：已实现

## 目标

为空列表/空会话提供统一插画；为 Agent 提供可选插画封面池，并在创建向导中可选。

## 决策

1. **自绘轻量 SVG**（React 组件），主色跟 `currentColor` / CSS 变量，不引入 unDraw 等第三方整库。
2. **空状态**：`EmptyIllustration` + scene id（chat / workspace / memory / cron / files / providers）。
3. **Agent 封面**：复用现有 **avatar** 槽写入封面 SVG；**emoji** 仍为 Lucide/小图标。`AgentAvatar` 已优先 avatar，卡片主视觉为封面，列表芯片同样可用。
4. **创建向导**：新增封面缩略图选择器 → `set_pending_agent_icon({ kind: "avatar", ... })`。
5. 不做 CDN/整包 npm；不新增 IDENTITY 字段（封面即 `assets/avatar.svg`）。

## 范围

- `apps/desktop/src/illustrations/`：registry + SVG 组件 + EmptyIllustration + CoverPicker
- 接入：ChatWelcome、Workspace/Memory/Cron/FileSpace/Providers 空态、AgentCreateGuide
- 样式：共享 `.astro-empty` / `.astro-cover-*`

## 非目标

- 运行时下载远程插画
- 后端新 cover 字段
