# 主界面导览

主界面使用 Driver.js 1.8 展示轻量导览，与首次启动的模型验证、工作区初始化分开。

- `OnboardingGate` 通过 `InterfaceTourReady` 通知 App 入场动画已经结束；入场期间不会弹导览。
- 本机尚无当前版本导览记录时，在聊天页空闲且原生浏览器面板已关闭后提供“带我了解 / 直接开始”。升级后的用户也只提示一次。
- 最多八个区域依次为输入框、模型选择、右上角工具栏、任务侧栏、工作区、插件、右下角壁纸/配色按钮、设置。每步可跳过；Esc 退出，左右方向键切换，Tab 在导览按钮间移动。
- 小风车在启用壁纸时切换最近壁纸（不足两张时打开外观设置），否则在灵动配色模式下重新配色；其他模式不展示按钮，导览也跳过该步。新增说明不重置已经完成或跳过的导览记录。
- 仅介绍当前布局中可见的区域，不打开工具、不发送消息、不修改模型或初始化配置。侧栏临时展开，结束后恢复偏好。不可见区域不会产生悬空高亮。
- 侧栏“界面导览”和“设置 → 关于 → 界面导览 → 重新查看”可重新启动。手动重看会回到聊天页并关闭浏览器面板，不清空对话或草稿。
- 侧栏底部将偏好设置、问号导览图标、爪印桌宠开关按顺序排列；展开时同排，窄图标栏时纵向排列。爪印读取真实窗口显隐状态，复用桌宠恢复/关闭接口，并跟随托盘、设置与全屏变化刷新；不重置造型、位置或大小。

## 保存边界

Tauri 命令 `get_interface_tour_state` / `resolve_interface_tour` 使用 `home::settings` 的共享文件锁和分段更新，在全局 `config.toml` 保存：

```toml
[desktop.interface_tour]
resolved_version = 1
outcome = "completed" # 或 skipped
```

完成和主动跳过都会停止自动提示；重看后跳过不会降级已经完成的状态，也不会降级更高版本。组件卸载、StrictMode 清理、界面暂时不可用不是“跳过”，不会写状态。读失败不阻塞主界面，手动入口仍可用；写失败显示可重试提示。

这不是 `ui/onboarding.json` 的初始化状态，也不是 WebView localStorage；清除浏览器存储不会重新开启已经结束的原生导览。

## 验证

```sh
cargo test -p astro-agent --lib commands::ui::interface_tour::tests
cd apps/desktop
node --test src/lib/ui/interfaceTour.test.ts
npx tsc --noEmit
npx playwright test interface-tour.spec.ts --workers=1
```

浏览器测试使用生产 App 和 OnboardingGate，仅替换原生 transport。它验证组件行为，不替代真实 Tauri WebView 的原生验收。
