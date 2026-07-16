# 对话右侧栏边缘对齐设计

## 目标

右侧对话侧栏保持浮层结构和宽度不变，其顶部、右侧与底部边缘和对话容器完全齐平。

## 实现

仅修改 `frontend/src/styles/features/chat/right-panel.css`：

- 将 `.chat-right-panel` 的 `top` 从 `10px` 改为 `0`。
- 将 `.chat-right-panel` 的 `right` 从 `10px` 改为 `0`。
- 将 `.chat-right-panel` 的 `bottom` 从 `10px` 改为 `0`。
- 保持宽度、圆角、阴影、遮罩和滑入动画不变。

## 验证

- 打开对话右侧栏，确认顶部、右侧与底部边缘和对话容器一致。
- 检查普通与展开对话模式。
- 确认侧栏内容仍可完整滚动，四角没有溢出。
- 运行前端构建检查。
