# 聊天与文件空间媒体预览

日期：2026-07-15  
状态：已实现  
关联：`2026-07-15-google-media-gen-tools-design.md`（生成落盘已通；本 spec 补 UI 预览）

## 背景

`image_gen` / `video_gen` / `tts` 已写入 `workspace/generated/`，工具结果为固定中文路径文案。聊天活动卡只渲染 `<pre>`；`ChatMarkdown` / A2UI Image 未把本地路径转为 Tauri asset URL；FileSpace 预览仅 text/image。结果：生成成功但气泡裂图或无播放器。

## 目标

1. 聊天内嵌预览生成图 / 视频 / 音频（工具结果卡 + Markdown / A2UI 本地路径）。
2. FileSpace / Workspace 预览图 / 视频 / 音频 / HTML（沙箱 iframe）。
3. HTML：fenced `html` 代码块与 `.html` 文件显式预览；不默认渲染任意 raw HTML。
4. 画板：仅 coming soon 入口，无交互实现。

## 非目标

- 自由绘图画板 / 协作
- 工具结果 JSON 化改协议
- Vision 真多模态
- 修改 `media_http.rs` 出站协议

## 架构

共享前端 kit：

| 单元 | 职责 |
|------|------|
| `resolveMediaSrc` | 绝对路径 / `file://` → `convertFileSrc`；其它可加载 URL 原样 |
| `parseGeneratedMedia` | 从工具 output 解析媒体 kind + path |
| `MediaPreview` | img / video / audio + Broken 占位 |
| `HtmlPreview` | sandbox iframe（`allow-scripts` only）+ 系统打开旁路 |

接入点：`MsgActivity`、`ChatMarkdown`、A2UI `CatalogAdapter`、`FileSpacePanel`、`WorkspacePanel`。

## 安全

- HTML iframe：`sandbox="allow-scripts"`，不含 `allow-same-origin` / `allow-top-navigation` / `allow-forms`
- 内容用 `srcDoc`（读入的本地 HTML 或代码块文本）
- Asset scope 沿用现有 `$HOME/**`；失败 → Broken + 系统打开

## 验收

1. `image_gen` 完成后气泡内直接出图；刷新后仍可从 activity output 路径显示
2. Markdown / A2UI 本地路径不再裂图
3. FileSpace / Workspace 可预览 mp4 / wav / mp3 / html
4. HTML 脚本可跑且无法跳出顶层
5. 画板仅见 coming soon
