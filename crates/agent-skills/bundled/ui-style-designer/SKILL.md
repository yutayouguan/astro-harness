---
name: ui-style-designer
description: 为 Astro 生成、切换或恢复桌面壁纸与完整界面主题。用户说“换壁纸”“生成某种风格主题”“应用配色”“恢复系统样式”等请求时使用；普通图片创作不使用。
astro_bundled_rev: 1
astro_tools: [request_user_input_async, image_gen, ui_style]
---

# Astro 界面风格

把自然语言意图转换为可回退的 Astro 壁纸或主题。不要写任意 CSS，也不要直接修改应用源码、localStorage 或 `~/.astro/ui/style`；只通过 `ui_style` 应用。

`image_gen` 和 `ui_style` 属于延迟工具；当前步骤未显示它们时，先用 `tool_search` 搜索并加载所需工具，再调用，不能用 shell 写清单来绕过。

## 先判断请求类型

- **只换壁纸**：保留现有界面配色与图标设置；用 `image_gen` 生成后，将返回的工作区相对路径传给 `ui_style action=apply` 的 `wallpaperPath`。除非用户明确要求，否则保持 `adaptiveColor=true`。
- **生成完整主题**：设计亮色与暗色两组受控 token，可选壁纸和图标动效，再用 `ui_style` 应用。
- **恢复上一套**：直接调用 `ui_style action=rollback`。
- **恢复系统样式**：直接调用 `ui_style action=reset`，不要重新生成资源。
- **查看当前样式**：调用 `ui_style action=status`。

## 信息不足时询问

生成前确认两个核心信息：视觉风格、壁纸主体/内容。缺少其中任一项且无法从上下文推断时，用一次 `request_user_input_async` 提问；每题提供 2～3 个互斥选项，并允许用户自由填写。用户说“你决定”“随便”“惊喜我”时自行选择，不再追问。

若按用户 locale、时区与当前日期判断，正处于常见节日前 14 天至节后 3 天，并且用户没有给出风格或内容，可在选项中加入“相关节日主题”；它只是建议，不能自动替用户选择。用户已经说明方向时，不要额外追问节日。

## 生成与应用

1. 壁纸使用横向桌面构图，主体避开中央阅读区，不要文字、品牌标志或水印；优先请求 16:9。
2. `image_gen` 成功后使用其原样返回的工作区相对路径，不猜测文件名。
3. `ui_style action=apply` 的 `lightTokens` / `darkTokens` 只使用受控颜色变量；优先选 `--color-accent`、`--color-accent-secondary`、`--color-bg-base`、`--color-bg-raised`、`--color-text`、`--color-text-muted`、`--color-border`、`--glass-fill` 和 `--glass-border`。亮暗主题都要保证正文、次级文字、边框与背景有清晰对比。
4. 图标动效仅选 `smooth`、`snappy`、`bouncy`；线宽仅选 `1`、`2`、`2.5`。
5. “换成 / 应用 / 设置为 / 给我生成一套主题”都表示已授权立即应用，不再要求二次确认。
6. 应用失败时保留当前样式并说明错误，不绕过校验写文件。成功时简短报告主题名以及壁纸、配色、图标中实际改变的部分。

每次 `apply` 都会自动保存上一份清单用于回退；无有效用户样式时 Desktop 使用现有系统主题和手动壁纸。
