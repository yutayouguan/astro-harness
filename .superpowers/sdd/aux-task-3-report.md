# Aux Task 3 Report: 辅助模型设置面板

## Scope
- Create: `frontend/src/hooks/settings/useAuxiliarySettings.ts`
- Create: `frontend/src/components/settings/AuxiliaryModelsPanel.tsx`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/lib/ui/navConfig.ts`
- Modify: `frontend/src/components/settings/index.ts`
- Modify: `frontend/src/hooks/chat/useChatSession.ts`
- Modify: `frontend/src/i18n/messages.ts`
- Modify: `frontend/src/styles/features/memory.css`

未做：实际标题/压缩/审批/入梦/审查调用点改造（Tasks 4-7）。

## What changed
- 新增 `useAuxiliarySettings`，封装 Task 2 的 `get/set/reset` Tauri 命令；mutation 成功后用返回的完整 settings 替换本地状态。
- 新增独立「辅助模型」设置页，固定展示五类任务：标题生成、上下文压缩、智能审批、入梦、记忆审查。
- 每行支持重置为主模型、选择启用 provider、从缓存/远端模型列表选择模型并提交 provider ID + model。
- 导航新增 `auxiliary` 独立入口；补齐中英文文案和基础样式。

## Verification
- `cd frontend && npm run build` PASS
- `ReadLints` on touched frontend files: no linter errors

## Self-review
- 面板不修改主模型配置，只写 `auxiliary.*` 路由。
- 模型列表优先缓存，空缓存再拉远端；拉取失败时保留 provider 默认模型作为可选项。
- 不可用提示来自后端 settings DTO；实际运行时仍由 resolver 回退主模型。
