# Astro 壁纸设置设计稿

## 设计目标

- 壁纸是全局 Shell 底层，不是聊天页的局部装饰。
- 背景来源与界面强调色分离，继续复用 Astro 现有 tone 与玻璃体系。
- 上传或 AI 生成的图片仅在成功校验后应用，失败不替换当前壁纸。
- AI 生成使用已配置的独立图片 Provider，默认生成横向画面。
- 设置侧边栏按“通用 / 智能体 / 扩展 / 系统”分组，子项使用清晰的能力名称，不与分组重名。

## 原型交互

- 切换“氛围配色 / 图片壁纸”。
- 上传本地图片并立即预览。
- 调整填充、内容保护与柔化。
- 打开 AI 生成对话框，体验生成中状态和成功后自动应用。
- 从最近使用列表切换壁纸。

## 设计参考

- `apps/desktop/src/components/settings/PreferencesPanel.tsx`
- `apps/desktop/src/styles/features/preferences.css`
- `apps/desktop/src/hooks/app/useShellColorStyle.ts`
- `apps/desktop/src/lib/ui/shellGradient.ts`

示例图片来自本机 macOS 系统壁纸缩略图，仅用于交互稿预览，不作为产品资源。
