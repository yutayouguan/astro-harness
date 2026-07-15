# 文件空间多类型查看与文本编辑

日期：2026-07-15  
状态：设计已确认，待实现计划

## 背景

文件空间右侧预览目前只支持：

- 文本：`read_file` → `<pre>`（无高亮、不可编辑）
- 图片：`convertFileSrc` → `<img>`

视频 / 音频归为 unsupported；HTML 只当源码字符串；PDF / 办公文档只能系统打开。工作区已有 `WorkspaceEditor`、`ChatMarkdown`、图/视频内嵌模式，技能侧有只读 `SkillFileViewer`，但文件空间未复用。

对话确认采用**方案 B**：新建 `FileSpaceViewer` 作为右侧内容入口，扩展 `fileTypeIcon` 的 open mode，列表逻辑留在 `FileSpacePanel`。

## 目标

1. 选中文件后，右侧能查看：文本/代码、Markdown、HTML、图片、视频、音频、PDF。
2. 文本类可编辑：防抖自动保存 + 手动保存（含 ⌘S），并有脏状态提示。
3. Markdown / HTML：预览 ↔ 源码切换；源码模式下预览随 draft 实时更新。
4. 打开方式判定与 `fileTypeIcon` 对齐，去掉 panel 内重复扩展名集合。

## 非目标

- Word / Excel / PPT 内嵌预览（二期）。
- 引入 pdf.js（除非实现时资产协议无法可靠加载 PDF，再补）。
- 把工作区改为自动保存，或合并 Workspace / FileSpace 为同一编辑器壳。
- 改 `media_http.rs`（出图/出视频 API，与本地预览无关）。
- 外部改文件冲突检测、版本历史、协作。
- 应用内媒体裁剪 / 转码。

## 架构

```
FileSpacePanel（列表 / 选中 / 批量）
        │ selected ArtifactDto
        ▼
FileSpaceViewer（右侧内容唯一入口）
        │ 按 resolveFileType + 扩展名分支
        ├─ TextEditorPane   → WorkspaceEditor + write_file
        ├─ MarkdownPane    → Editor + ChatMarkdown（源码态带实时预览）
        ├─ HtmlPane        → Editor + sandboxed iframe (srcdoc)
        ├─ MediaPane       → img / video / audio + convertFileSrc
        └─ PdfPane         → iframe/embed + 失败回退系统打开
fileTypeIcon.ts            → 增加 media-audio / media-pdf
```

## 类型判定

以 `resolveFileType(name).open` 为主，删除 `FileSpacePanel` 内 `TEXT_EXTS` / `IMAGE_EXTS`。

扩展 `FileOpenMode`：

| mode | 含义 | 示例扩展名 |
|------|------|------------|
| `text` | 文本编辑（含 html/md 的进一步分支） | txt, rs, ts, json, html, md… |
| `media-image` | 图片 | png, jpg, webp, svg… |
| `media-video` | 视频 | mp4, webm, mov… |
| `media-audio`（新增） | 音频 | mp3, wav, m4a, flac, ogg, aac… |
| `media-pdf`（新增） | PDF | pdf |
| `external` | 系统打开 | docx, xlsx, pptx, zip… |

Viewer 内部分支：

- `.md` / `.markdown` / `.mdx` → MarkdownPane
- `.html` / `.htm` → HtmlPane
- 其它 `text` → TextEditorPane

判定优先级：basename 特殊名 → 扩展名 → artifact `mime`（若有 `image/*` / `text/*` 等）→ `external`。

## 交互

### 打开

选中 → Viewer 按类型加载。媒体 / PDF 用 `convertFileSrc`；文本类用 `read_file`。失败则提示，并保留「系统打开 / 在文件夹中显示」。

### 文本 / 代码

始终编辑器。头栏：文件名、脏标记、保存按钮。`⌘S` / `Ctrl+S` 立即写盘。

### Markdown / HTML

工具栏：

- **预览**：只读（MD → `ChatMarkdown`；HTML → `sandbox` iframe + `srcdoc`）
- **源码**：`WorkspaceEditor` + 旁侧或下方**实时预览**（同一 draft，随输入更新）

HTML iframe：`sandbox` 允许脚本以便本地产物可预览；禁止导航顶层窗口。外部资源按浏览器默认，不做代理。

### 图片 / 视频 / 音频

居中内嵌；视频 / 音频带原生 `controls`。

### PDF

优先 `convertFileSrc` + `<iframe>` / `<embed>`。加载失败则提示 + 「系统打开」。

## 保存规则

| 规则 | 说明 |
|------|------|
| 自动保存 | draft 变更后防抖约 600ms 调用 `write_file` |
| 手动保存 | 按钮或 ⌘S：取消防抖并立即写 |
| 脏状态 | `draft !== lastSaved` 时显示未保存 |
| 切换文件 | 先 flush 待写自动保存；失败则确认「丢弃 / 重试」 |
| 关闭面板 | 同切换：尽量 flush |

## 错误处理

| 场景 | 行为 |
|------|------|
| 文件缺失 | missing 态 + 文案 |
| `read_file` 失败 | 错误提示 + 「系统打开」 |
| `write_file` 失败 | 保留 draft，不假装已保存；toast 或预览区错误 |
| 媒体 / PDF URL 失败 | 占位说明 + 「系统打开」 |
| 非 Tauri | 媒体 / PDF 不可用时明确提示；文本编辑尽量可用 |

## 样式与 i18n

- 扩展 `filespace.css` 的 preview / viewer 区块。
- MD/HTML 模式切换视觉可对齐 `.ws-md-mode`，本期不强制抽共享组件。
- 新增未保存、保存中、预览/源码、媒体失败等文案 key。

## 预期文件清单

| 文件 | 变更 |
|------|------|
| `frontend/src/components/FileSpaceViewer.tsx` | 新建（可同文件分 pane） |
| `frontend/src/components/FileSpacePanel.tsx` | 接入 Viewer，移除旧 PreviewState / 扩展名集合 |
| `frontend/src/lib/fileTypeIcon.ts` | `media-audio` / `media-pdf` |
| `frontend/src/lib/fileTypeIcon.test.ts` | 覆盖新 mode |
| `frontend/src/styles/filespace.css` | viewer / 媒体 / 分栏样式 |
| i18n messages | 新增相关 key |

## 测试要点

1. `fileTypeIcon`：音频 → `media-audio`，pdf → `media-pdf`；回归图/视频。
2. Viewer 分支：各类型进入正确 pane。
3. 保存：防抖触发 `write_file`；⌘S 立即写；切换文件前 flush。
4. MD/HTML：源码编辑后预览更新；预览模式只读。
5. 手工：Tauri 下图 / 音 / 视 / PDF 各打开一例。

## 二期（不在本期）

- Word / Excel / PPT 内嵌预览（挂在同一 `FileSpaceViewer`）。
- 若 PDF 资产协议不可靠，再评估 pdf.js。
- 视需要将 Workspace 保存策略与 FileSpace 对齐。
