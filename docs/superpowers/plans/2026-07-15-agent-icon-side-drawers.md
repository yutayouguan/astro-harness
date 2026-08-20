# Agent Icon Side Drawers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 创建 Agent 引导卡的头像与 Emoji 选择改为两个互斥的右侧抽屉；卡片仅保留预览入口。

**Architecture:** 新建 `AvatarPickerDrawer`（预设插画 + 上传）；将 `LucideIconPicker` 从居中弹层改为右侧抽屉壳；`AgentCreateGuide` 改为两行入口并管理 `avatarOpen` / `lucideOpen` 互斥。pending icon / cover→avatar 逻辑仍留在 Guide。

**Tech Stack:** React + 现有 CSS（对齐 `mcp-add-drawer`）、i18n `messages.ts`、现有 `CoverPicker` / Tauri `set_pending_agent_icon`

**Spec:** `docs/superpowers/specs/2026-07-15-agent-icon-side-drawers-design.md`

## Global Constraints

- 不改后端 pending icon / IDENTITY 协议
- 不复用 `ChatRightPanel`；不抽通用 SideDrawer
- 头像 → `assets/avatar.*`；Emoji → `assets/emoji.*`
- 抽屉 z-index ≥ mcp-add（1300）；宽度 `min(420px, 92vw)`，全高贴右
- 仓库无组件测试：验收用手动清单 + `tsc`/现有相关单元测试不回归

## File map

| File | Role |
|------|------|
| `apps/desktop/src/components/AvatarPickerDrawer.tsx` | 新建：头像右侧抽屉 |
| `apps/desktop/src/components/LucideIconPicker.tsx` | 壳改为右侧抽屉；可加可选上传回调 |
| `apps/desktop/src/components/AgentCreateGuide.tsx` | 两行入口 + 开抽屉 + 互斥 |
| `apps/desktop/src/styles/chat.css` | `.agent-icon-drawer-*` + Lucide 壳样式改侧滑 |
| `apps/desktop/src/i18n/messages.ts` | zh/en 抽屉与入口文案 |
| Spec status | 标为已批准 |

---

### Task 1: i18n 文案

**Files:**
- Modify: `apps/desktop/src/i18n/messages.ts`（zh + en）

**Produces:** 下列 MessageKey（写入 `zh` 后 `en` 必须齐全）

- [x] **Step 1: 在 zh 中增加/调整键**

```ts
"chat.agentAvatarDrawerTitle": "选择头像",
"chat.agentAvatarDrawerSub": "选内置插画或上传自定义图，写入 assets/avatar",
"chat.agentAvatarDrawerUpload": "上传自定义图片",
"chat.agentAvatarDrawerUploadHint": "会覆盖上方插画预设",
"chat.agentIconPick": "选择",
"chat.agentIconChange": "更换",
// 调整总述（若尚未到位）
"chat.agentIconsSub": "头像用于对话主视觉；Emoji 是小号 Lucide。点右侧入口在抽屉中选择。",
"chat.agentCoversTitle": "头像",
"chat.agentCoversSub": "对话与列表优先展示",
"chat.agentIconEmoji": "Emoji 小图标",
"chat.agentIconEmojiHint": "侧栏等处的小图标",
```

- [x] **Step 2: 为 en 写对齐翻译**

```ts
"chat.agentAvatarDrawerTitle": "Choose avatar",
"chat.agentAvatarDrawerSub": "Pick a built-in illustration or upload; saved as assets/avatar",
"chat.agentAvatarDrawerUpload": "Upload custom image",
"chat.agentAvatarDrawerUploadHint": "Overrides the illustration preset",
"chat.agentIconPick": "Choose",
"chat.agentIconChange": "Change",
"chat.agentIconsSub": "Avatar for chat face; Emoji is a small Lucide mark. Open the side drawer to pick.",
"chat.agentCoversTitle": "Avatar",
"chat.agentCoversSub": "Preferred in chat and lists",
"chat.agentIconEmoji": "Emoji icon",
"chat.agentIconEmojiHint": "Small mark for sidebars and lists",
```

保留已有 `chat.lucidePicker*`、`chat.agentIconUpload` / `Clear` / `Replace` 等。

- [x] **Step 3: Commit**

```bash
git add apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
i18n(chat): 头像/Emoji 侧抽屉文案

EOF
)"
```

---

### Task 2: 共用右侧抽屉 CSS + Lucide 改壳

**Files:**
- Modify: `apps/desktop/src/styles/chat.css`（`lucide-picker-*` 段 + 新 `agent-icon-drawer-*`）
- Modify: `apps/desktop/src/components/LucideIconPicker.tsx`

**Consumes:** Task 1 文案（lucide 标题仍用原 key）  
**Produces:** 侧滑抽屉壳；Lucide 打开时为右侧面板

- [x] **Step 1: 在 `chat.css` 增加共用壳（对齐 mcp-add）**

在 Lucide 段之前加入：

```css
.agent-icon-drawer-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1300;
  display: flex;
  justify-content: flex-end;
  background:
    radial-gradient(
      ellipse 55% 70% at 88% 30%,
      color-mix(in srgb, var(--tone-blue) 18%, transparent),
      transparent 65%
    ),
    color-mix(in srgb, var(--bg-base, #0f172a) 32%, transparent);
  backdrop-filter: blur(10px) saturate(1.2);
  -webkit-backdrop-filter: blur(10px) saturate(1.2);
}

html[data-theme="light"] .agent-icon-drawer-backdrop {
  background:
    radial-gradient(
      ellipse 55% 70% at 88% 30%,
      color-mix(in srgb, var(--tone-blue) 22%, transparent),
      transparent 65%
    ),
    color-mix(in srgb, #64748b 18%, transparent);
}

.agent-icon-drawer {
  position: relative;
  isolation: isolate;
  width: min(420px, 92vw);
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-left: 1px solid color-mix(in srgb, var(--tone, var(--tone-blue)) 22%, var(--glass-edge));
  background:
    linear-gradient(155deg, rgba(255, 255, 255, 0.78), rgba(255, 255, 255, 0.32)),
    color-mix(in srgb, var(--tone-soft, var(--tone-blue-soft)) 55%, var(--glass-panel, rgba(255, 255, 255, 0.5)));
  box-shadow:
    -16px 0 48px color-mix(in srgb, var(--tone, var(--tone-blue)) 14%, rgba(15, 23, 42, 0.1)),
    var(--glass-rim);
  backdrop-filter: blur(22px) saturate(1.3);
  -webkit-backdrop-filter: blur(22px) saturate(1.3);
  animation: agent-icon-drawer-in 0.22s cubic-bezier(0.22, 1, 0.36, 1) both;
}

@keyframes agent-icon-drawer-in {
  from { transform: translateX(100%); opacity: 0.6; }
  to { transform: translateX(0); opacity: 1; }
}

.agent-icon-drawer-head {
  position: relative;
  z-index: 1;
  flex-shrink: 0;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 16px 16px 14px;
  border-bottom: 1px solid color-mix(in srgb, var(--tone, var(--tone-blue)) 8%, transparent);
}

.agent-icon-drawer-scroll {
  position: relative;
  z-index: 1;
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 12px 16px 20px;
}

.agent-icon-drawer-foot {
  position: relative;
  z-index: 1;
  flex-shrink: 0;
  padding: 12px 16px 16px;
  border-top: 1px solid color-mix(in srgb, var(--glass-edge) 80%, transparent);
}
```

暗色：对照 `mcp-add-drawer` dark 规则用 `html[data-theme="dark"] .agent-icon-drawer` 写一套。

- [x] **Step 2: 改 `LucideIconPicker` 外壳 class**

将根结构从居中 panel 改为：

```tsx
<div
  className="agent-icon-drawer-backdrop"
  role="presentation"
  onMouseDown={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
>
  <div
    ref={panelRef}
    className="agent-icon-drawer lucide-picker-drawer"
    style={{ ["--tone" as string]: "var(--tone-blue)", ["--tone-soft" as string]: "var(--tone-blue-soft)" }}
    role="dialog"
    aria-modal="true"
    aria-label={t("chat.lucidePickerTitle")}
  >
    <header className="agent-icon-drawer-head lucide-picker-head">…</header>
    <div className="agent-icon-drawer-scroll lucide-picker-scroll">
      {/* search / style / colors / gradients / grid 原内容 */}
    </div>
  </div>
</div>
```

可选 props 扩展（若 Guide 要把「上传图片」放进抽屉）：

```ts
type Props = {
  // …existing
  /** 若提供，抽屉内显示上传按钮并回调 File */
  onUploadImage?: (file: File) => void;
};
```

上传按钮放在 scroll 顶部或 head 旁；选中图标仍调用 `onSelect`（Guide 内关抽屉）。

- [x] **Step 3: 调整旧 `.lucide-picker-backdrop/.panel`**

删除或改为兼容注释；抽屉内网格用 `.lucide-picker-drawer .lucide-picker-grid { … }` 保证可滚、max-height 取消（由 scroll 容器负责）。

- [x] **Step 4: 手动冒烟**

在已开 `tauri dev` 的创建流程里：临时点开 Lucide（若入口未改可用旧按钮）应看到右侧滑入而非居中弹层。

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src/styles/chat.css apps/desktop/src/components/LucideIconPicker.tsx
git commit -m "$(cat <<'EOF'
feat(chat): Lucide 选择器改为右侧抽屉壳

EOF
)"
```

---

### Task 3: `AvatarPickerDrawer`

**Files:**
- Create: `apps/desktop/src/components/AvatarPickerDrawer.tsx`
- Modify: `apps/desktop/src/styles/chat.css`（上传区小样式）

**Consumes:** Task 1 keys；Task 2 `.agent-icon-drawer-*`；`CoverPicker`  
**Produces:**

```ts
type AvatarPickerDrawerProps = {
  open: boolean;
  coverId: CoverId | null;
  busy?: boolean;
  onClose: () => void;
  onPickCover: (id: CoverId) => void;
  onUpload: (file: File) => void;
};
```

- [x] **Step 1: 实现组件**

```tsx
/** Agent 头像选择：右侧抽屉（预设插画 + 上传）。 */
import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import { CoverPicker, type CoverId } from "../illustrations";

type Props = {
  open: boolean;
  coverId: CoverId | null;
  busy?: boolean;
  onClose: () => void;
  onPickCover: (id: CoverId) => void;
  onUpload: (file: File) => void;
};

export default function AvatarPickerDrawer({
  open,
  coverId,
  busy,
  onClose,
  onPickCover,
  onUpload,
}: Props) {
  const { t } = useI18n();
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  if (!open) return null;

  return (
    <div
      className="agent-icon-drawer-backdrop"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div
        className="agent-icon-drawer"
        style={{ ["--tone" as string]: "var(--tone-purple)", ["--tone-soft" as string]: "var(--tone-purple-soft)" }}
        role="dialog"
        aria-modal="true"
        aria-labelledby="agent-avatar-drawer-title"
      >
        <header className="agent-icon-drawer-head">
          <div>
            <h3 id="agent-avatar-drawer-title" className="lucide-picker-title">
              {t("chat.agentAvatarDrawerTitle")}
            </h3>
            <p className="lucide-picker-sub">{t("chat.agentAvatarDrawerSub")}</p>
          </div>
          <button
            type="button"
            className="lucide-picker-close"
            onClick={onClose}
            disabled={busy}
            aria-label={t("chat.lucidePickerClose")}
          >
            <X size={16} />
          </button>
        </header>
        <div className="agent-icon-drawer-scroll">
          <CoverPicker
            value={coverId}
            busy={busy}
            onChange={onPickCover}
          />
        </div>
        <footer className="agent-icon-drawer-foot">
          <p className="chat-agent-covers-sub">{t("chat.agentAvatarDrawerUploadHint")}</p>
          <button
            type="button"
            className="chat-agent-icon-btn"
            disabled={busy}
            onClick={() => inputRef.current?.click()}
          >
            {t("chat.agentAvatarDrawerUpload")}
          </button>
          <input
            ref={inputRef}
            type="file"
            accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml,.png,.jpg,.jpeg,.webp,.gif,.svg"
            hidden
            onChange={(e) => {
              const file = e.target.files?.[0] ?? null;
              e.target.value = "";
              if (file) onUpload(file);
            }}
          />
        </footer>
      </div>
    </div>
  );
}
```

- [x] **Step 2: Commit**

```bash
git add apps/desktop/src/components/AvatarPickerDrawer.tsx apps/desktop/src/styles/chat.css
git commit -m "$(cat <<'EOF'
feat(chat): 新增头像选择右侧抽屉

EOF
)"
```

---

### Task 4: 改写 `AgentCreateGuide` 入口 + 接线

**Files:**
- Modify: `apps/desktop/src/components/AgentCreateGuide.tsx`

**Consumes:** AvatarPickerDrawer；LucideIconPicker；现有 `pick` / `pickCover` / `pickLucide` / `clear`

- [x] **Step 1: 状态与互斥**

```tsx
const [avatarOpen, setAvatarOpen] = useState(false);
const [lucideOpen, setLucideOpen] = useState(false);

const openAvatar = () => {
  setLucideOpen(false);
  setAvatarOpen(true);
};
const openLucide = () => {
  setAvatarOpen(false);
  setLucideOpen(true);
};
```

- [x] **Step 2: 外观区改为两行入口（伪结构）**

```tsx
<section className="chat-agent-icons">
  <div className="chat-agent-icons-head">…</div>

  {/* 头像行：预览 + 选择/清除 */}
  <div className="chat-agent-icon-slot tone-avatar is-row">
    <button type="button" className="chat-agent-icon-preview …" onClick={openAvatar}>
      {avatar.previewUrl ? <img … /> : <span className="chat-agent-icon-fallback">{initial}</span>}
    </button>
    <div className="chat-agent-icon-slot-meta">
      <span className="chat-agent-icon-slot-label">{t("chat.agentCoversTitle")}</span>
      <span className="chat-agent-icon-slot-hint">{t("chat.agentCoversSub")}</span>
      <div className="chat-agent-icon-actions">
        <button type="button" className="chat-agent-icon-btn" onClick={openAvatar}>
          {avatar.previewUrl ? t("chat.agentIconChange") : t("chat.agentIconPick")}
        </button>
        {avatar.previewUrl ? (
          <button type="button" className="chat-agent-icon-btn subtle" onClick={() => void clear("avatar")}>
            {t("chat.agentIconClear")}
          </button>
        ) : null}
      </div>
    </div>
  </div>

  {/* Emoji 行：同上，openLucide；预览用 emoji.previewUrl */}
</section>

<AvatarPickerDrawer
  open={avatarOpen}
  coverId={coverId}
  busy={coverBusy}
  onClose={() => setAvatarOpen(false)}
  onPickCover={(id) => {
    void pickCover(id).then(() => setAvatarOpen(false));
  }}
  onUpload={(file) => {
    void pick("avatar", file).then(() => setAvatarOpen(false));
  }}
/>

<LucideIconPicker
  open={lucideOpen}
  selectedId={emoji.lucideId}
  onClose={() => setLucideOpen(false)}
  onSelect={(icon, paint, style) => {
    void pickLucide(icon, paint, style); // 内部已 setLucideOpen(false)
  }}
  onUploadImage={(file) => {
    void pick("emoji", file).then(() => setLucideOpen(false));
  }}
/>
```

删除内嵌 `CoverPicker` 与双 `renderSlot` 网格；`renderSlot` 可删或留作私有辅助。

注意：`pickCover` / `pick` 当前返回 Promise；在成功路径关抽屉。`pickLucide` 已 `setLucideOpen(false)`。

- [x] **Step 3: 手动验收（对照 spec）**

1. 创建卡无插画网格，仅两行  
2. 开头像抽屉 → 选插画 → 预览更新、抽屉关  
3. 再开 → 上传图 → 覆盖、关  
4. 开 Emoji 抽屉 → 选 Lucide / 上传 → 预览更新、关  
5. Esc / 遮罩可关；开一头像时 Lucide 应关（互斥）  
6. 亮/暗色可读  

- [x] **Step 4: Commit**

```bash
git add apps/desktop/src/components/AgentCreateGuide.tsx apps/desktop/src/components/LucideIconPicker.tsx
git commit -m "$(cat <<'EOF'
feat(chat): 创建 Agent 头像/Emoji 改右侧抽屉选择

EOF
)"
```

- [x] **Step 5: 更新 spec 状态**

将 `docs/superpowers/specs/2026-07-15-agent-icon-side-drawers-design.md` 顶部状态改为「已实现」，链到本 plan。

```bash
git add docs/superpowers/specs/2026-07-15-agent-icon-side-drawers-design.md
git commit -m "$(cat <<'EOF'
docs: 标记头像/Emoji 侧抽屉设计为已实现

EOF
)"
```

---

## Spec coverage self-check

| Spec 项 | Task |
|---------|------|
| 卡片两行入口、无内嵌网格 | 4 |
| 头像抽屉：预设+上传 | 3 |
| Lucide 右侧抽屉 | 2 |
| 选中后关抽屉 | 3–4 |
| 互斥 | 4 |
| Esc/遮罩 | 2–3 |
| z-index / 宽度 | 2 |
| i18n | 1 |
| 不改后端 | 全程 |

## Placeholder scan

无 TBD / 「类似 Task N」悬空引用。
