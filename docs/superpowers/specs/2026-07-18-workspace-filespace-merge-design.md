# 工作空间与文件空间合并

日期：2026-07-18  
状态：已实现（主路径完成；FileSpace 窄列 viewer 未强行统一到共享内容组件）

## 背景

此前左侧主导航有两个互斥全页：

| 导航项 | 组件 | 数据源 | 能力 |
|--------|------|--------|------|
| 工作空间 | `WorkspacePanel` | 实时 `list_files`（当前 Agent `workspace_dir`） | 目录树、CRUD、剪切粘贴、编辑/预览 |
| 文件空间 | `FileSpacePanel` + `FileSpaceViewer` | SQLite 产物索引（`list_artifacts` / `reconcile`） | 分类筛选、会话分组、回到会话、附加到聊天 |

两者物理文件有重叠（workspace 文件会被 reconcile 编入索引），但数据模型与独有能力不同，不宜二选一删除。用户目标：**合并为一个导航入口**，内部分「浏览 / 产物」两种模式，并统一预览体验与双向跳转。

## 目标

1. 左侧导航合并为单一项「工作空间」（`nav = files`），页头提供「浏览 / 产物」分段切换。
2. 切换子模式时**保留各自状态**（滚动、选中、已打开文件），不做卸载重建。
3. 抽出共享文件元信息与预览内容组件，减少重复实现。
4. 双向跳转：
   - 产物 → 在工作区中打开（定位目录并打开文件）
   - 浏览文件 → 回到来源会话（按路径查产物索引）
5. 产物视图主题跟随「工作空间」tab 的紫色 tone。

## 非目标

- 统一两套数据源（`list_files` 与 `list_artifacts` 仍分层）。
- 强行把 `FileSpaceViewer` 窄列布局改成整页 `FilePreviewContent` 样式（会破坏 split 实时预览与窄栏视觉）。
- 把工作区保存策略改成与产物侧完全一致（浏览仍为手动 Save；产物侧仍为防抖自动保存）。
- 改产物索引 schema 或 reconcile 扫描范围。

## 架构

```
nav = "files"
        │
        ▼
   App 页头：files-mode-switch（浏览 | 产物）
        │  submode 持久化 astro.files.submode
        ▼
   FilesPage（两侧常驻挂载，CSS 显隐）
        ├─ WorkspacePanel（浏览）── FilePreviewContent
        └─ FileSpacePanel（产物）── FileSpaceViewer（保留窄列专用布局）
                 │
                 ├─ onOpenInWorkspace(path) → 切 browse + openPath
                 └─ WorkspacePanel.onOpenSession ← find_artifact_by_path
```

### 关键文件

| 路径 | 职责 |
|------|------|
| [apps/desktop/src/lib/ui/navConfig.ts](../../../apps/desktop/src/lib/ui/navConfig.ts) | `NavId` 含 `files`；`PAGE_META.files` |
| [apps/desktop/src/components/files/FilesPage.tsx](../../../apps/desktop/src/components/files/FilesPage.tsx) | 外壳：显隐切换、透传 `openPath` / `onOpenSession` |
| [apps/desktop/src/lib/filespace/filesMode.ts](../../../apps/desktop/src/lib/filespace/filesMode.ts) | `FilesSubmode` + localStorage |
| [apps/desktop/src/lib/filespace/fileMeta.ts](../../../apps/desktop/src/lib/filespace/fileMeta.ts) | 共享 `formatSize` / `isTauri` |
| [apps/desktop/src/components/filespace/FileGlyph.tsx](../../../apps/desktop/src/components/filespace/FileGlyph.tsx) | 共享文件类型图标 |
| [apps/desktop/src/components/filespace/FilePreviewContent.tsx](../../../apps/desktop/src/components/filespace/FilePreviewContent.tsx) | 按类型渲染预览/编辑内容区 |
| [apps/desktop/src-tauri/src/artifacts_commands.rs](../../../apps/desktop/src-tauri/src/artifacts_commands.rs) | `find_artifact_by_path` |

## 行为说明

### 导航与分段

- 导航文案：`nav.files` / `page.files.title` =「工作空间」；副标题说明可浏览文件或查看产物。
- 分段切换在**共享页头右侧**（与 chat 的 ModelPicker 同排），不在内容区再叠一层 bar。
- 子模式持久化 key：`astro.files.submode`（`browse` | `artifacts`）。

### 状态保留

- `FilesPage` 两侧面板常驻挂载；隐藏侧使用 `.files-mode-pane.is-hidden { display: none }`。
- `FileSpacePanel` 仅在产物可见时 `active=true`，避免后台 reconcile/轮询。
- `WorkspacePanel.openPath` 随 prop 变化消费（不再依赖重挂载）。

### 预览与工具栏（浏览模式）

- HTML/Markdown：同一 editor 视图内用 `mdMode` 切换预览/源码，工具栏不换页。
- 预览/源码：`ws-md-modes` 分段，纯图标（Eye / FileCode2）。
- 打开/下载/复制等：`MediaToolbar` 内联到顶部工具栏；HTML 预览去掉自带标题栏。
- 媒体（图/视/音）：内容平铺填满舞台；顶部共用图标工具栏。

### 双向跳转

| 方向 | 入口 | 实现 |
|------|------|------|
| 产物 → 浏览 | 右键「在工作区中打开」 | `onOpenInWorkspace` → `changeFilesMode("browse")` + `openPath` |
| 浏览 → 会话 | 工具栏「回到来源会话」（有关联会话时显示） | `invoke("find_artifact_by_path")` → `onOpenSession(sessionId, messageId)` |

仅当文件在产物索引中且带 `session_id` 时显示「回到来源会话」。

### 主题

- `.filespace-panel` 的 `--tone` / `--tone-soft` 由 cyan 改为 purple，与合并后导航项 tone 一致。

## 清理

已删除合并后无引用项：

- i18n：`nav.workspace`、`nav.filespace`、`page.workspace.*`、`page.filespace.*`（中英）
- 图标：`IconFileSpace`

## 取舍与后续

| 项 | 决策 | 后续可选 |
|----|------|----------|
| FileSpace 预览壳 | 保留窄列 `FileSpaceViewer`（含宽屏 split） | 若接受视觉变化，可改用 `FilePreviewContent` 并去掉 split |
| 保存策略 | 浏览手动 / 产物自动，未强行统一 | 给共享 pane 加 `saveStrategy` prop |
| 反向跳转查库 | 复用 `get_by_path`，路径需与索引一致 | 可补 canonicalize / 模糊匹配 |

## 相关提交（节选）

- `feat(files): 合并「工作空间」与「文件空间」为单个「文件」导航`
- `refactor(files): 浏览/产物分段切换移到页头`
- `style(files): 产物视图主题跟随「文件」tab（青→紫）`
- `i18n(files): 「文件」tab 更名为「工作空间」`
- `feat(files): 切换浏览/产物时保留各自状态`
- `feat(files): 浏览文件 → 回到来源会话`
- `chore(files): 清理合并后无用的 i18n 与图标`
- `refactor(files): 抽出共享 FilePreviewContent，WorkspacePanel 改用`
