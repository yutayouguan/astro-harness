import { useRef, useState, type ReactNode } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Bell, Folder, Settings, Sparkles } from "lucide-react";
import {
  Button,
  Drawer,
  IconButton,
  ModalShell,
  PopoverSurface,
  SegmentedTabs,
  Surface,
  type ButtonSize,
  type ButtonVariant,
} from "../components/ui";

const variants: ButtonVariant[] = ["primary", "secondary", "ghost", "danger"];
const sizes: ButtonSize[] = ["sm", "md", "lg"];

function StoryCanvas({ children }: { children: ReactNode }) {
  return (
    <main
      style={{
        minHeight: "100vh",
        padding: 40,
        background: "var(--shell-bg)",
        color: "var(--color-text)",
      }}
    >
      <div style={{ width: "min(960px, 100%)", margin: "0 auto" }}>{children}</div>
    </main>
  );
}

function PrimitiveGallery() {
  const [tab, setTab] = useState("overview");
  return (
    <StoryCanvas>
      <h1 style={{ margin: "0 0 24px", fontSize: 26 }}>UI 原语状态矩阵</h1>
      <Surface variant="elevated" style={{ marginBottom: 20 }}>
        <h2 style={{ margin: "0 0 16px", fontSize: 15 }}>Button / IconButton</h2>
        <div style={{ display: "grid", gap: 12 }}>
          {variants.map((variant) => (
            <div key={variant} style={{ display: "flex", gap: 10, alignItems: "center" }}>
              {sizes.map((size) => (
                <Button key={size} variant={variant} size={size}>
                  {variant} · {size}
                </Button>
              ))}
              <Button variant={variant} active>Active</Button>
              <Button variant={variant} busy>Busy</Button>
              <Button variant={variant} disabled>Disabled</Button>
              <IconButton variant={variant} aria-label={`${variant} 通知`}>
                <Bell size={16} aria-hidden />
              </IconButton>
            </div>
          ))}
        </div>
      </Surface>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
          gap: 16,
          marginBottom: 20,
        }}
      >
        {(["panel", "card", "elevated"] as const).map((variant) => (
          <Surface key={variant} variant={variant} interactive tabIndex={0}>
            <strong>{variant}</strong>
            <p style={{ margin: "8px 0 0", color: "var(--color-text-muted)" }}>
              统一边框、表面与交互反馈。
            </p>
          </Surface>
        ))}
      </div>

      <Surface variant="panel">
        <h2 style={{ margin: "0 0 14px", fontSize: 15 }}>SegmentedTabs</h2>
        <div style={{ display: "flex", gap: 18, flexWrap: "wrap" }}>
          <SegmentedTabs
            aria-label="工作区视图"
            value={tab}
            onValueChange={setTab}
            items={[
              { value: "overview", label: "概览", icon: <Sparkles size={14} />, count: 8 },
              { value: "files", label: "文件", icon: <Folder size={14} />, count: 24 },
              { value: "settings", label: "设置", icon: <Settings size={14} /> },
              { value: "disabled", label: "禁用", disabled: true },
            ]}
          />
          <SegmentedTabs
            aria-label="紧凑工作区视图"
            size="sm"
            value={tab}
            onValueChange={setTab}
            items={[
              { value: "overview", label: "概览" },
              { value: "files", label: "文件" },
              { value: "settings", label: "设置" },
            ]}
          />
        </div>
      </Surface>
    </StoryCanvas>
  );
}

function ModalStory() {
  const [open, setOpen] = useState(true);
  const initialFocusRef = useRef<HTMLInputElement | null>(null);
  return (
    <StoryCanvas>
      <Button variant="primary" onClick={() => setOpen(true)}>打开 Modal</Button>
      <ModalShell
        open={open}
        onClose={() => setOpen(false)}
        initialFocusRef={initialFocusRef}
        aria-label="示例模态框"
      >
        <h2 style={{ marginTop: 0 }}>ModalShell</h2>
        <p style={{ color: "var(--color-text-secondary)" }}>
          Escape、背景点击、焦点恢复与 Tab 焦点环均由 Overlay 原语统一处理。
        </p>
        <input ref={initialFocusRef} aria-label="初始焦点示例" placeholder="初始焦点" />
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
          <Button onClick={() => setOpen(false)}>取消</Button>
          <Button variant="primary" onClick={() => setOpen(false)}>确认</Button>
        </div>
      </ModalShell>
    </StoryCanvas>
  );
}

function DrawerStory() {
  const [open, setOpen] = useState(true);
  const initialFocusRef = useRef<HTMLButtonElement | null>(null);
  return (
    <StoryCanvas>
      <Button variant="primary" onClick={() => setOpen(true)}>打开 Drawer</Button>
      <Drawer
        open={open}
        onClose={() => setOpen(false)}
        size="lg"
        initialFocusRef={initialFocusRef}
        backdropStyle={{
          background:
            "radial-gradient(circle at 90% 20%, rgba(14, 165, 233, 0.18), rgba(15, 23, 42, 0.42))",
        }}
        aria-label="示例抽屉"
      >
        <h2 style={{ marginTop: 0 }}>Drawer</h2>
        <p style={{ color: "var(--color-text-secondary)" }}>适用于上下文编辑和详情检查。</p>
        <Button ref={initialFocusRef} onClick={() => setOpen(false)}>关闭</Button>
      </Drawer>
    </StoryCanvas>
  );
}

function PopoverStory() {
  const [open, setOpen] = useState(true);
  const anchorRef = useRef<HTMLButtonElement | null>(null);
  return (
    <StoryCanvas>
      <Button ref={anchorRef} variant="primary" onClick={() => setOpen((value) => !value)}>
        切换 Popover
      </Button>
      <PopoverSurface
        open={open}
        onClose={() => setOpen(false)}
        anchorRef={anchorRef}
        aria-label="示例浮层"
      >
        <strong>PopoverSurface</strong>
        <p style={{ margin: "8px 0 12px", color: "var(--color-text-secondary)" }}>
          复用现有锚点定位逻辑，并在视口边界内自动翻转。
        </p>
        <Button size="sm" onClick={() => setOpen(false)}>完成</Button>
      </PopoverSurface>
    </StoryCanvas>
  );
}

const meta = {
  title: "Design/UI Primitives",
  component: PrimitiveGallery,
  parameters: { controls: { disable: true } },
} satisfies Meta<typeof PrimitiveGallery>;

export default meta;
type Story = StoryObj<typeof meta>;

export const StateMatrix: Story = {};
export const OpenModal: Story = { render: () => <ModalStory /> };
export const OpenDrawer: Story = { render: () => <DrawerStory /> };
export const OpenPopover: Story = { render: () => <PopoverStory /> };
