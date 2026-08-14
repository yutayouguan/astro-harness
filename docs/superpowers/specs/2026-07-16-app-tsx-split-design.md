# App.tsx 分步拆分

日期：2026-07-16  
状态：第 1 刀已批准（待实现）

## 目标

将约 3300 行的 `apps/desktop/src/App.tsx` 从「壳 + 聊天运行时 + 面板编排」拆成可维护结构，**不改变 UI / 运行时行为**。

## 总体策略（三刀）

| 阶段 | 内容 | 非目标 |
|------|------|--------|
| **1** | 抽出纯函数与导航配置 | 不抽 hook、不拆 JSX |
| **2** | 抽出 chat 运行时（`useChatSession` 等） | 不上路由、不大改面板 |
| **3** | 侧栏 / 标题栏等 shell 组件化，`App` 只编排 | — |

每阶段独立可验证、可 commit；完成第 1 刀后再细化第 2 / 3 刀设计。

## 第 1 刀：纯函数 + 配置

### 文件归属

| 从 `App.tsx` 迁出 | 目标路径 |
|------------------|----------|
| `MAX_ATTACHMENTS`, `MAX_INLINE_BYTES`, `kindFromMime` | `apps/desktop/src/lib/chat/attachments.ts` |
| `ACTIVITY_KINDS`, `countChatBubbles`, `mapHistoryMessages` | `apps/desktop/src/lib/chat/historyMap.ts` |
| `calcTokensPerSec` | `apps/desktop/src/lib/chat/tokensPerSec.ts` |
| `NavId`, `IconComp`, `Tone`, `StatusPhase`, `NAV`, `PAGE_META` | `apps/desktop/src/lib/ui/navConfig.ts` |

说明：`navConfig` 放 `lib/ui/`（与现有 `windowZoom` / `windowUnderlay` 等同属壳层工具），不新建 `lib/shell/`。

### App.tsx 变更

- 删除上述本地定义
- 改为从新模块 import
- 其余逻辑（state、effect、JSX）不动

### 测试

- `kindFromMime`：MIME / 扩展名 → image|video|audio|file
- `calcTokensPerSec`：边界（0 token、0 duration）与正常值
- `mapHistoryMessages`：过滤非 user/assistant；activity kind/status 归一；uiSurfaces status 归一
- `countChatBubbles`：排除 welcome

测试文件放对应 `*.test.ts`（与现有 `lib/chat/*.test.ts` 一致）。运行方式与仓库一致：

```bash
cd frontend && node --experimental-strip-types --test \
  src/lib/chat/attachments.test.ts \
  src/lib/chat/historyMap.test.ts \
  src/lib/chat/tokensPerSec.test.ts
```

### 验收

1. `cd frontend && npm run build` 通过
2. 新单测通过
3. 无行为 / UI 变更

## 第 2 / 3 刀（占位，本刀不实施）

- **第 2 刀**：将消息流、`listen` AG-UI、发停、压实、HITL 等迁入 `hooks/useChatSession.ts`（可再拆子 hook），`App` 只消费返回的 state / handlers。
- **第 3 刀**：`Sidebar`、标题栏拖拽区等抽成 `components/` 或 `shell/` 组件，`App` 变薄编排层。

细节在第 1 刀落地后另开设计补充。

## 非目标（全文）

- 不引入路由框架
- 不拆 `App.tsx` 以外的已域化 components（除非第 3 刀明确需要）
- 不改 Tauri / 后端协议
