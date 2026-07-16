# Frontend components 按域重组

日期：2026-07-16  
状态：已批准（待实现）

## 目标

将 `frontend/src/components/` 从「几乎全平铺 + 仅 `media/` 成组」整理为按功能域分目录，降低找文件成本，并约定新组件落点。

## 决策

1. **按功能域分目录**（域优先，不用 panels/pickers 等技术类型分层）。
2. **参照 `illustrations/`**：域内文件 + 可选 `index.ts` barrel；外部可用 barrel 或深路径。
3. **一次改全仓库 import**，不做根目录永久兼容 re-export。
4. **`git mv` 分域迁移**，每域一轮可验证（`tsc` / 前端构建），多 commit 完成；默认同分支一次做完，不强制每域单独 PR。
5. **`hooks/`、`lib/`、`contexts/`、`a2ui/`、`App.tsx` 拆分** 不在本次范围。

## 目标结构

```
frontend/src/components/
  chat/
  filespace/
  workspace/
  agents/
  schedule/
  settings/
  media/       # 已有，路径不变
  ui/
  icons/
```

## 文件归属

| 目录 | 文件 |
|------|------|
| `chat/` | `ChatView`, `ChatWelcome`, `ChatSessionList`, `ChatRightPanel`, `ChatMessageNav`, `ChatMarkdown`, `ChatContextTimeline`, `ChatAgentInfo`, `MsgTimeline`, `MsgStreamLoader`, `MsgReasoning`, `MsgDissolveOverlay`, `MsgActivity`, `ComposerPalette`, `ComposerMcpMenu`, `ContextExplorer`, `ContextUsageBar`, `ContextUsagePopover`, `A2UISurfaceCard` |
| `filespace/` | `FileSpacePanel`, `FileSpaceViewer`, `FileSpaceBatchBar`, `FileSpaceConfirm`, `FileContextMenu` |
| `workspace/` | `WorkspacePanel`, `WorkspaceEditor`, `WorkspaceBatchBar`, `WorkspaceIcons` |
| `agents/` | `AgentPicker`, `AgentAvatar`, `AgentCreateGuide`, `AvatarPickerDrawer`, `ModelPicker`, `ModelCapabilityIcons`, `LucideIconPicker` |
| `schedule/` | `CronPanel`, `CreateCronDialog`, `ScheduleEditor` |
| `settings/` | `PreferencesPanel`, `ProvidersPanel`, `ToolsPanel`, `SkillsPanel`, `SkillFileViewer`, `MemoryPanel`, `InsightsPanel`, `SidebarContextMenu` |
| `media/` | 现有 5 文件不动 |
| `ui/` | `Toast`, `SelectMenu`, `AnimatedSwitch`, `ExpandableSearch` |
| `icons/` | `NavIcons`, `ProviderIcons`, `ToolIcons`, `GlassSolidIcons`, `McpIcon`, `LucideByName` |

### 归属细则

- `ModelCapabilityIcons` → `agents/`（与选模型绑定），不进 `icons/`
- `WorkspaceIcons` → `workspace/`
- `SidebarContextMenu` → `settings/`
- `LucideIconPicker` → `agents/`
- 仅被多域复用的通用控件进 `ui/`；跨域图标进 `icons/`

## 迁移顺序

1. `icons`
2. `ui`
3. `chat`
4. `filespace`
5. `workspace`
6. `agents`
7. `schedule`
8. `settings`

每步：`git mv` → 更新相对 import 与全仓库引用 → 类型检查 / 构建通过 → commit。

## 新组件约定

1. 先落入对应功能域目录。
2. 被 2+ 域复用再抽到 `ui/` 或 `icons/`。
3. 可选在域内 `index.ts` 重导出公开入口。

## 非目标

- 不改变运行时行为或 UI
- 不拆 `App.tsx`
- 不挪动 `hooks/`、`lib/`、`contexts/`、`a2ui/`
- 不引入路径别名大改（沿用现有相对路径 / 既有 alias 习惯）
