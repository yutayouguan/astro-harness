import { useRef, useState, type CSSProperties } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ChatWelcome } from "../components/chat/ChatWelcome";
import { useTheme } from "../hooks/app/useTheme";
import { useI18n } from "../i18n/LocaleContext";
import softMeta from "./SoftMaterial.stories";

function SoftWelcomePreview() {
  const { setMode, setMaterial, material } = useTheme();
  const { locale, setLocale } = useI18n();
  const [draft, setDraft] = useState("");
  const [hints, setHints] = useState<string[]>([]);
  const [texture, setTexture] = useState(false);
  const editor = useRef<HTMLTextAreaElement>(null);
  return (
    <div
      style={
        {
          height: "100%",
          display: "flex",
          flexDirection: "column",
          background: texture
            ? "repeating-linear-gradient(135deg, transparent 0 28px, rgba(56, 139, 183, 0.24) 28px 32px), var(--soft-base)"
            : undefined,
          "--composer-overlay-height": "0px",
        } as CSSProperties
      }
    >
      <nav
        aria-label="样板设置"
        style={{ display: "flex", gap: 10, flexWrap: "wrap" }}
      >
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
        <button
          type="button"
          onClick={() => setLocale(locale === "zh" ? "en" : "zh")}
        >
          中文 / English
        </button>
        <button
          type="button"
          aria-pressed={texture}
          onClick={() => setTexture((value) => !value)}
        >
          纹理背景（仅样板）
        </button>
      </nav>
      <ChatWelcome
        onActivate={() => editor.current?.focus()}
        onPickCard={(prompt, slotHints) => {
          setDraft(prompt);
          setHints(slotHints);
          editor.current?.focus();
        }}
      />
      <footer
        className="composer composer--stacked"
        style={{
          flexShrink: 0,
          margin: 0,
          padding: "10px 16px",
          background: "var(--surface-panel-background)",
          borderRadius: 12,
        }}
      >
        <textarea
          ref={editor}
          className="composer-input"
          aria-label="提示词草稿"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          rows={2}
        />
        <small data-testid="slot-hints">{hints.join(" · ")}</small>
      </footer>
    </div>
  );
}
const meta = {
  title: "Design/Soft Welcome",
  component: SoftWelcomePreview,
  decorators: softMeta.decorators,
  beforeEach: softMeta.beforeEach,
} satisfies Meta<typeof SoftWelcomePreview>;
export default meta;
type Story = StoryObj<typeof meta>;
export const ContentCards: Story = {};
