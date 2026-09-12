import { useEffect, useState } from "react";
import { PawPrint } from "lucide-react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import ModelPicker from "../components/agents/ModelPicker";
import { ThemeProvider, useTheme } from "../hooks/app/useTheme";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { DialogProvider } from "../hooks/ui/DialogContext";
import type { ProviderDto } from "../types";
import "../styles/features/shell/layout/sidebar-footer-actions.css";

const provider: ProviderDto = {
  id: "accent-fixture",
  kind: "openai",
  display_name: "Preview",
  endpoint: "",
  model: "gpt-preview",
  enabled: true,
  has_api_key: false,
  key_source: "none",
  env_key_name: null,
  backend_id: "preview",
};

function ChromeThemeAccent() {
  const { mode, setMode } = useTheme();
  const [tone, setTone] = useState("#7c3aed");
  const [palette, setPalette] = useState("unified");
  const [visible, setVisible] = useState(true);
  const [model, setModel] = useState(provider.model);
  useEffect(() => {
    const root = document.documentElement;
    const attrs = ["data-color-style", "data-tone", "data-wallpaper-palette"];
    const previous = attrs.map((name) => root.getAttribute(name));
    const vars = ["--unified-tone", "--wallpaper-tone"];
    const values = vars.map((name) => root.style.getPropertyValue(name));
    root.dataset.colorStyle = palette === "wallpaper" ? "unified" : palette;
    root.dataset.tone = "blue";
    root.dataset.wallpaperPalette = String(palette === "wallpaper");
    root.style.setProperty("--unified-tone", tone);
    root.style.setProperty("--wallpaper-tone", tone);
    return () => {
      attrs.forEach((name, i) =>
        previous[i] === null
          ? root.removeAttribute(name)
          : root.setAttribute(name, previous[i]!),
      );
      vars.forEach((name, i) =>
        values[i]
          ? root.style.setProperty(name, values[i])
          : root.style.removeProperty(name),
      );
    };
  }, [tone, palette]);
  return (
    <main
      className="app-shell"
      data-tone="blue"
      style={{
        minHeight: "100vh",
        padding: 32,
        boxSizing: "border-box",
        display: "block",
      }}
    >
      <p>隔离配色样板：模型使用模拟数据，宠物按钮只切换样板状态。</p>
      <label>
        主题颜色
        <select
          aria-label="主题颜色"
          value={tone}
          onChange={(e) => setTone(e.target.value)}
        >
          <option value="#7c3aed">紫色</option>
          <option value="#15803d">绿色</option>
        </select>
      </label>
      <label>
        配色来源
        <select
          aria-label="配色来源"
          value={palette}
          onChange={(e) => setPalette(e.target.value)}
        >
          <option value="unified">统一</option>
          <option value="dynamic">灵动</option>
          <option value="wallpaper">壁纸</option>
        </select>
      </label>
      <button onClick={() => setMode(mode === "dark" ? "light" : "dark")}>
        切换明暗
      </button>
      <div
        className="content-header"
        data-tone="blue"
        style={{ marginTop: 32 }}
      >
        <div className="header-actions">
          <ModelPicker
            providers={[{ ...provider, model }]}
            value={provider.id}
            onChange={(_, next) => setModel(next)}
          />
        </div>
      </div>
      <div
        className="sidebar-footer sidebar-footer-actions"
        style={{ width: 240, marginTop: 40 }}
      >
        <span>偏好设置</span>
        <button
          className="sidebar-settings-btn sidebar-footer-icon"
          data-sidebar-action="pet"
          aria-label="样板宠物开关"
          aria-pressed={visible}
          onClick={() => setVisible(!visible)}
        >
          <PawPrint size={17} strokeWidth={1.8} />
        </button>
      </div>
    </main>
  );
}

const meta = {
  title: "Design/Chrome Theme Accent",
  component: ChromeThemeAccent,
  decorators: [
    (Story) => (
      <ThemeProvider>
        <LocaleProvider>
          <MorphiconProvider>
            <DialogProvider>
              <Story />
            </DialogProvider>
          </MorphiconProvider>
        </LocaleProvider>
      </ThemeProvider>
    ),
  ],
  beforeEach: () => {
    mockIPC((command) => {
      if (
        command === "get_cached_provider_models" ||
        command === "list_provider_models"
      )
        return { models: [{ id: "gpt-preview" }, { id: "gemini-preview" }] };
      throw new Error(`Read-only accent preview: ${command}`);
    });
    return clearMocks;
  },
} satisfies Meta<typeof ChromeThemeAccent>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
