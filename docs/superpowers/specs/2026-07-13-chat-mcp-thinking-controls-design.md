# 智能对话：MCP 快捷开关与推理 / Auto·MAX 控件收敛

**日期:** 2026-07-13  
**状态:** 已批准  
**范围:** 聊天 Composer MCP 弹出层；ModelPicker 隐藏 Auto / MAX；推理按钮按模型能力显示

## 目标

在智能对话输入栏提供 MCP 服务快捷开关（含跳转设置），去掉模型选择器中的 Auto / MAX Mode，并仅在当前模型支持推理时显示推理强度控件。

## 背景与约束

- Composer 已有模式 pill、推理 pill（现仅 `backend_id === "deepseek"`）、`@` / `/`。
- MCP 状态由 `useMcpTools` 管理，与 Tools 面板同源；enabled 决定对话是否带上该服务。
- ModelPicker 顶部有 Auto / MAX Mode；`App` 发送路径在 `globals.auto` 时走 `autoModelSelect`。
- 用户决策：MCP = **本地 enabled 开关 + 底部进设置**；推理判定 = **caps.reasoning 优先，未知则回退 provider 白名单**；Auto/MAX = **隐藏 UI，加载时强制关掉并写回**。

## 方案选择

采用 **Composer 独立 MCP 弹出层 + 复用 `useMcpTools`**（方案 A），而非塞进现有 Palette（Palette 不适配多开关），亦非仅跳转入口，也不做 Cursor 式「+」总菜单重构。

## UI 与交互

### Composer（左→右）

1. 对话模式 pill（不变）
2. 推理 pill（仅 `showThinking` 为真时显示；级别 off / low / high / max 不变）
3. **MCP 图标按钮**（有任一 enabled 服务时可显示小圆点）
4. `@`、`/`（不变）

### MCP 弹出层

- 搜索框过滤服务名
- 列表：名称 + toggle → `toggleServer(id)`
- 空态：无服务提示
- 底部：「打开 MCP 设置」→ `onOpenMcpSettings()` → `setNav("tools")` 且 Tools 初始 tab=`mcp`
- 点击外部 / Esc 关闭；与 thinking / mention / slash 互斥

### ModelPicker

- 删除 Auto / MAX 开关及 Auto 提示文案区
- 触发器始终显示当前具体模型（不再显示「Auto」）
- `loadPickerGlobals()` 规范化：`auto=false`、`maxMode=false` 并写回存储

## 数据流与判定

### MCP

- 弹出层与 ToolsPanel 共用同一 agent 作用域下的 MCP 状态（`useMcpTools(agentId)`）
- 不改变后端协议；发送仍只带 enabled 服务
- ToolsPanel 增加可选初始 tab（如 `initialTab` / 一次性 focus），保证从聊天跳入时落在 MCP

### 推理按钮

```
showThinking =
  activeModel.capabilities.reasoning === true
  || (caps 未知 && activeProvider.backend_id ∈ {"deepseek"})
```

- 不支持推理时隐藏按钮；**不改本地 thinking 偏好存储**
- 发送侧：若不支持推理，忽略 thinking（不向后端传 enabled thinking）

### Auto / MAX

- 读全局偏好时强制关闭并持久化
- 发送路径中 `globals.auto` 分支实际不可达；实现阶段优先小改（可保留死分支）

## 错误处理

| 场景 | 行为 |
|------|------|
| MCP toggle 写盘失败 | 保持原状态；沿用 hook 现有错误处理 |
| 跳转 Tools 时 panel 未就绪 | 先切 nav，再用 prop / 一次性标记指定 mcp tab |
| 模型 caps 未加载 | 仅走 provider 白名单回退 |
| Auto 迁移 | 静默写回，不弹窗 |

流式发送中：MCP toggle 仍可操作；推理按钮保持现有 disabled 行为。

## 验收

1. 聊天栏 MCP 按钮可弹出、搜索、开关；底栏可进 MCP 设置 tab
2. ModelPicker 无 Auto / MAX；旧偏好被关掉后显示具体模型名
3. 仅 caps.reasoning 或 deepseek 回退时显示推理 pill
4. 开关 MCP 后下一轮对话工具集随之变化
5. 中英 i18n 齐全

## 非目标

- 不在弹出层内启停 MCP 操作系统进程
- 不重做 Tools 面板 MCP 管理 UI
- 不新增 Auto 选模替代方案
