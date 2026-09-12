import { useRef, useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import AppDialog, { type AppDialogVariant } from "../components/ui/AppDialog";
import { SelectMenu } from "../components/ui/SelectMenu";
import { PopoverSurface } from "../components/ui/Overlay";
import { Toast, type ToastTone } from "../components/ui/Toast";
import SidebarContextMenu from "../components/settings/SidebarContextMenu";
import { useTheme } from "../hooks/app/useTheme";
import softMeta from "./SoftMaterial.stories";

function SoftOverlays() {
  const { material, setMaterial, setMode } = useTheme();
  const [dialog, setDialog] = useState<AppDialogVariant | null>(null);
  const [name, setName] = useState("柔塑主题样板");
  const [selection, setSelection] = useState("a");
  const [toast, setToast] = useState<ToastTone | null>(null);
  const [popover, setPopover] = useState(false);
  const [context, setContext] = useState<{ x: number; y: number } | null>(null);
  const [result, setResult] = useState("尚未操作");
  const anchor = useRef<HTMLButtonElement>(null);
  const finish = (message: string) => {
    setResult(message);
    setDialog(null);
  };
  return (
    <main style={{ padding: 24, maxWidth: 720, display: "grid", gap: 24 }}>
      <header>
        <h2>柔塑浮层</h2>
        <p>以下操作只修改样板状态，不操作真实数据。</p>
      </header>
      <div style={{ display: "flex", gap: 10 }}>
        <button type="button" onClick={() => setMode("light")}>
          亮色
        </button>
        <button type="button" onClick={() => setMode("dark")}>
          暗色
        </button>
        <button
          type="button"
          onClick={() => setMaterial(material === "soft" ? "glass" : "soft")}
        >
          材质：{material}
        </button>
      </div>
      <SelectMenu
        aria-label="可搜索下拉"
        value={selection}
        onChange={setSelection}
        options={[
          { value: "a", label: "白瓷亮色" },
          { value: "b", label: "石墨暗色" },
          { value: "c", label: "跟随系统" },
        ]}
        search={{ placeholder: "筛选样板选项", emptyLabel: "没有匹配项" }}
      />
      <div style={{ display: "flex", gap: 12, flexWrap: "wrap" }}>
        <button type="button" onClick={() => setDialog("default")}>
          确认弹窗
        </button>
        <button type="button" onClick={() => setDialog("danger")}>
          危险操作样板
        </button>
        <button type="button" onClick={() => setDialog("prompt")}>
          输入弹窗
        </button>
        <button type="button" ref={anchor} onClick={() => setPopover(true)}>
          轻浮层
        </button>
        <button
          type="button"
          onClick={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            setContext({ x: rect.left, y: rect.bottom });
          }}
        >
          侧栏菜单
        </button>
      </div>
      <div style={{ display: "flex", gap: 12 }}>
        <button type="button" onClick={() => setToast("success")}>
          成功通知
        </button>
        <button type="button" onClick={() => setToast("warning")}>
          警告通知
        </button>
        <button type="button" onClick={() => setToast("error")}>
          错误通知
        </button>
      </div>
      <p role="status">{result}</p>
      <AppDialog
        open={dialog !== null}
        variant={dialog ?? "default"}
        title={
          dialog === "danger"
            ? "危险操作样板（不删除数据）"
            : dialog === "prompt"
              ? "输入名称"
              : "确认操作"
        }
        message="检查表面、语义色和键盘操作。"
        confirmLabel="确认样板"
        cancelLabel="取消"
        confirmOnEnter={dialog !== "danger"}
        trapFocus
        onCancel={() => finish("已取消")}
        onConfirm={() => finish(`已确认：${name}`)}
      >
        {dialog === "prompt" ? (
          <input
            className="app-dialog-input"
            aria-label="样板名称"
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        ) : null}
      </AppDialog>
      <PopoverSurface
        open={popover}
        onClose={() => setPopover(false)}
        anchorRef={anchor}
        aria-label="轻浮层样板"
      >
        <p>浮层外不遮暗，内容使用实色表面。</p>
        <button type="button" onClick={() => setPopover(false)}>
          关闭轻浮层
        </button>
      </PopoverSurface>
      {context ? (
        <SidebarContextMenu
          {...context}
          labelsVisible
          pinned
          onClose={() => setContext(null)}
          onAction={(action) => {
            setResult(action);
            setContext(null);
          }}
        />
      ) : null}
      <Toast
        visible={toast !== null}
        tone={toast ?? "info"}
        message="这是通知样板，未修改真实数据。"
        onDismiss={() => setToast(null)}
      />
    </main>
  );
}
const meta = {
  title: "Design/Soft Overlays",
  component: SoftOverlays,
  decorators: softMeta.decorators,
  beforeEach: softMeta.beforeEach,
} satisfies Meta<typeof SoftOverlays>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
