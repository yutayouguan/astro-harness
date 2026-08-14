# 聊天媒体预览与会话产物关联

日期：2026-07-18  
状态：已实现  
相关：[generated 按类型分子目录](./2026-07-15-generated-typed-subdirs-design.md)、[聊天媒体预览](./2026-07-15-chat-media-preview-design.md)

## 背景

会话中 Agent 生成的 HTML / 图片 / 音视频 / 代码 / PDF 会以多媒体卡片展示在聊天中。此前存在三类问题：

1. **预览体验不完整**：卡片内 HTML 预览宽度受限、工具栏重复、无法在 App 内全屏查看；相对路径图片在沙箱 iframe 中加载失败。
2. **产物会话归属断裂**：生成文件多数经文件空间 `reconcile` 扫盘入库，`session_id=None`，被归入「未关联会话」。
3. **路径形态不一致风险**：`file_ops` 经 `resolve_safe` canonicalize 后登记，与 reconcile / 媒体 sidecar 的非规范化路径可能重复入库。

## 目标

1. 聊天多媒体卡片标题栏提供统一工具操作：引用 / **App 内预览** / 系统打开 / 下载 / 复制。
2. App 内全屏预览覆盖：网页、图片、视频、音频、代码、PDF。
3. HTML 内嵌预览能正确加载工作区内相对资源（含 `../`）。
4. 会话中生成的产物实时写入 `artifacts.db`，带 `session_id`；历史未关联文件可在 reconcile 时 best-effort 回填。
5. 登记路径形态与 reconcile 一致，避免重复行。

## 非目标

- 精确滚动到「定位」按钮对应的助手气泡（工具消息行号与 UI 气泡 id 不同套；打开会话仍可用）
- 对非 PDF 的 `document`（docx 等）做 App 内渲染
- 自动关联从未写入 `media_json` 的历史文件
- 改造离线进化 / 设置页开关（另见全局扁平开关样式提交）

## 已确认决策

| 项 | 选择 |
|----|------|
| 全屏预览入口 | 放在共享 `MediaToolbar`，所有媒体卡片复用 |
| 系统打开 | 图标按钮进工具栏；卡片内 HTML 自带标题栏隐藏，避免双工具栏 |
| HTML 相对资源 | ★ 逐引用改写为绝对 `convertFileSrc` URL；`<base href>` 仅作兜底 |
| 实时关联注入点 | ★ `AgentLoop::record_tool_result_with_id`（有 session / message / media） |
| 普通写文件 | `file_ops` write/append 额外登记；`project_root` 模式跳过 |
| 历史修复 | reconcile 后按 `messages.media_json` 回填 `session_id IS NULL` 行 |
| 路径形态 | 非规范化 `workspace_dir.join(rel)`，与 reconcile 对齐 |

## 架构总览

```text
┌──────────────────── 前端（聊天） ────────────────────┐
│ GeneratedMediaCard                                   │
│   └─ MediaToolbar                                    │
│        ├─ quote / preview / open / download / copy   │
│        └─ MediaPreviewModal（全屏）                   │
│             ├─ HtmlPreview / CodeFileCard / iframe   │
│             └─ video / audio / image                 │
└──────────────────────────────────────────────────────┘

┌──────────────────── 后端（生成落盘） ─────────────────┐
│ 媒体工具 → astro_media_v1 sidecar                    │
│ file_ops write/append → 可选 HTML sidecar + 登记     │
│        ↓                                             │
│ AgentLoop::record_tool_result_with_id                │
│   ├─ append_message(+ media_json)                    │
│   └─ register_media_artifacts(session_id, msg_id)    │
│        ↓                                             │
│ artifacts.db（AgentWrite）                           │
└──────────────────────────────────────────────────────┘

┌──────────────────── 文件空间 ────────────────────────┐
│ reconcile_artifacts                                  │
│   ├─ reconcile 扫盘（补漏 / missing）                │
│   └─ backfill_artifact_sessions（历史 session 回填） │
└──────────────────────────────────────────────────────┘
```

## 1. 聊天媒体卡片与工具栏

### 组件职责

| 组件 | 路径 | 职责 |
|------|------|------|
| `GeneratedMediaCard` | `apps/desktop/src/components/media/GeneratedMediaCard.tsx` | 外层卡片：文件名 + 工具栏 + 内嵌预览 |
| `MediaToolbar` | `apps/desktop/src/components/media/MediaToolbar.tsx` | 共享工具条 |
| `MediaPreview` | `apps/desktop/src/components/media/MediaPreview.tsx` | 按 kind 渲染内嵌预览 |
| `HtmlPreview` | `apps/desktop/src/components/media/HtmlPreview.tsx` | 沙箱 HTML（`srcDoc`） |
| `MediaPreviewModal` | `apps/desktop/src/components/media/MediaPreviewModal.tsx` | App 内全屏预览浮层 |

### 工具栏能力矩阵

| 操作 | 条件 | 行为 |
|------|------|------|
| 引用 | image / html / code / document，且聊天附件上下文可用 | `attachMediaPath` |
| 预览 | html / code；document 且 `.pdf`；image/video/audio 且可解析 src | 打开 `MediaPreviewModal` |
| 系统打开 | 始终（有本地 path） | `open_path_externally` |
| 下载 / 复制 | 始终 | `downloadMedia` / `copyMedia` |

### UI 收敛规则（CSS）

卡片上下文中避免双层工具栏：

- `.gen-media-card .gen-media-card-preview.html-preview`：宽度 `100%`，去掉内嵌预览的 `640px` 上限。
- `.gen-media-card .…html-preview .html-preview-bar`：`display: none`（操作已上移到卡片头）。
- iframe 默认高度约 `460px`（紧凑模式 `320px`）。

## 2. App 内全屏预览

`MediaPreviewModal` 通过 `createPortal` 挂到 `document.body`：

- Esc / 点遮罩 / 关闭按钮退出；打开时锁定 `body` 滚动。
- 按 `kind` + 路径后缀分发：

| kind / 条件 | 渲染 |
|-------------|------|
| `html` | `HtmlPreview`（铺满 stage，隐藏内嵌 bar） |
| `code` | `CodeFileCard`（只读 CodeMirror，内部滚动） |
| `document` 且 `.pdf` | `iframe` + `resolveMediaSrc` |
| `video` | `<video controls autoPlay>` |
| `audio` | `GlassAudioPlayer` |
| 其它可解析 src | `<img>` |

文案复用现有 i18n：`filespace.preview`、`media.zoomClose`。

## 3. HTML 相对资源加载

### 问题

1. `srcDoc` 文档基址是 `about:srcdoc`，相对路径无 base。
2. 仅注入 `<base href={convertFileSrc(dir)}>` 不够：`convertFileSrc` 会把整段路径 `encodeURIComponent`（`/` → `%2F`），HTML 里的 `../` 会弹掉该唯一 path 段，退回 asset 根，落不进 `assetProtocol.scope`。

### 方案（★）

渲染前流水线（`HtmlPreview`）：

1. `rewriteHtmlRelativeAssets(html, dir, convertFileSrc)`  
   - 改写 `src` / `poster` 与 CSS `url(...)` 中的相对引用。  
   - 按文件目录解析 `..` 后，对**绝对本地路径**逐个 `convertFileSrc`。  
   - 纯函数，单测见 `apps/desktop/src/lib/media/htmlAssetRewrite.test.ts`。
2. `withBaseHref(...)` 仍注入 `<base>` 作兜底（文档已有 `<base>` 则不覆盖）。
3. iframe `sandbox="allow-scripts"`（无 `same-origin` / top-nav）。

## 4. 产物会话关联

### 4.1 实时路径（媒体 sidecar）

注入点：`crates/agent-core/src/runtime/mod.rs` → `record_tool_result_with_id`

1. `common::extract_tool_media(content)` 解析 `astro_media_v1` sidecar。
2. `append_message` 落库工具消息（含 `media_json`），得到 `msg_id`。
3. `register_media_artifacts(&media, msg_id)`：
   - 仅处理 `MediaRef::WorkspacePath`
   - 路径：`memory.workspace_dir.join(rel)`（**非** canonicalize）
   - `ArtifactSource::AgentWrite` + `session_id` + `message_id` + `agent_id`
   - 失败仅 `debug` 日志，不阻断工具流

覆盖：`image_gen` / `tts` / `music_gen` / `video_gen` / HTML `file_ops`（带 sidecar）等。

### 4.2 实时路径（普通写文件）

注入点：`crates/agent-tools/src/builtin/shell/file_ops.rs` → `register_workspace_artifact`

- 在 `write` / `append` 落盘成功后调用。
- **跳过** `project_root`（委派 worktree / 代码仓）与空 `session_id`。
- 登记路径刻意用 `workspace_dir.join(rel)`，避免与 `resolve_safe` 的 canonicalize 结果分叉。
- HTML 仍可能登记两次（file_ops 一次、`record_tool_result` 一次）；同 path UPSERT + `COALESCE`，无重复行，message_id 会被后写补全。

### 4.3 历史回填

入口：`reconcile_artifacts`（打开文件空间时触发）

```text
reconcile(扫盘) → backfill_artifact_sessions
```

`backfill_artifact_sessions`：

1. `ArtifactDb::unlinked_paths()`：`session_id IS NULL AND missing=0`
2. `SessionStore::media_messages()`：跨会话 `(session_id, message_id, media_json)`
3. 按 basename 建索引，再以完整相对路径后缀校验命中
4. `link_session_by_path`：**仅更新** `session_id IS NULL` 的行

### 4.4 路径一致性（防重复行）

| 登记方 | 路径形态 |
|--------|----------|
| 媒体 sidecar 登记 | `workspace_dir.join(rel)` 非规范化 |
| file_ops 登记 | 同上（不用 `resolve_safe` 的 canonicalize full） |
| reconcile 扫盘 | `walkdir` 的 `to_string_lossy()` 非规范化 |

`ON CONFLICT(path)` + `session_id = COALESCE(excluded, existing)`：后续 reconcile 的 `None` 不会冲掉已有关联。

### 4.5 message_id 语义（已知限制）

- 存的是**工具消息**行号（`messages.id`）。
- 前端历史气泡 id 是**用户/助手**消息行号；工具结果折叠进助手气泡，且可能 `coalesce_consecutive_assistants`。
- 因此「定位」按钮：能打开正确会话，**不一定**精确滚动高亮；分组与「继续会话」不依赖 message_id。

## 5. 关键文件索引

| 区域 | 文件 |
|------|------|
| 工具栏 / 全屏预览 | `apps/desktop/src/components/media/MediaToolbar.tsx`、`MediaPreviewModal.tsx` |
| HTML 预览与改写 | `HtmlPreview.tsx`、`apps/desktop/src/lib/media/htmlAssetRewrite.ts` |
| 卡片样式 | `apps/desktop/src/styles/components/media.css` |
| 实时登记 | `crates/agent-core/src/runtime/mod.rs`、`tools/.../file_ops.rs` |
| 回填 / IPC | `apps/desktop/src-tauri/src/artifacts_commands.rs`、`artifacts/src/db.rs`、`crates/agent-session/src/store/messages.rs` |

## 6. 验证要点

### 前向（新生成）

1. 会话内生成图片 / HTML / 音频 → 聊天卡片可全屏预览。
2. HTML 含同目录或 `../images/...` 相对图 → 预览内可见。
3. 打开文件空间 → 文件出现在**当前会话分组**，source 倾向 `agent_write`。
4. `file_ops` 写普通文本 → 文件空间归属当前会话（聊天里无媒体卡，符合预期）。

### 回填（历史）

1. 打开文件空间触发 reconcile。
2. 库内曾有 `media_json` 记录的未关联文件应被 `link_session_by_path` 关联。
3. 从未出现在任何 `media_json` 中的文件仍保持未关联（无依据可关联）。

### 回归注意

- 卡片内不应再出现第二套下载/复制工具栏。
- 文件空间独立 HTML 预览仍保留内嵌标题栏（无外层卡片头）。
- `project_root` 下的 write 不进 artifacts。

## 7. 后续可选

| 项 | 说明 | 优先级 |
|----|------|--------|
| 精确定位 | 登记最近助手消息 id，或前端按 media path 反查 activity | 低 |
| 连接复用 | file_ops / agent 避免每次 `open_default` | 低 |
| 回填短路 | 仅在 `added>0` 或首次打开时跑 backfill | 低 |
| 非 PDF document | 系统打开或第三方渲染 | 低 |

## 附录：相关提交（本轮主线）

- `feat(media): 工具栏新增全屏预览…` / 代码 / PDF 支持
- `fix(html-preview): 修复内嵌 HTML 相对图片被 asset 协议拒绝`（`htmlAssetRewrite`）
- `fix(artifacts): 会话中生成的文件实时关联 session_id`
- `fix(artifacts): 回填历史「未关联」文件的会话归属`
- `fix(file_ops): 普通写文件也关联当前会话`
- `fix(file_ops): 产物登记路径与 reconcile 统一，避免重复行`
