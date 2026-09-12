import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState, type CSSProperties } from "react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import PreferencesPanel from "../components/settings/PreferencesPanel";
import { ChatMarkdown } from "../components/chat/ChatMarkdown";
import { ThemeProvider, useTheme } from "../hooks/app/useTheme";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import preferencesMeta from "./PreferencesPanel.stories";
import sampleWallpaper from "../assets/generated/desktop-pet-concept.png";
import { resolveWallpaperPresentation } from "../lib/ui/activeUiStyle";
import "./soft-material-sample.css";

function SoftMaterialSample() {
  const { mode, setMode } = useTheme();
  const [wallpaper, setWallpaper] = useState(false);
  const [wallpaperBlur, setWallpaperBlur] = useState(0);
  const wallpaperPrefs = {
    ...preferencesMeta.args.wallpaper.prefs,
    mode: wallpaper ? ("wallpaper" as const) : ("color" as const),
    current: wallpaper
      ? {
          id: "soft-material-sample",
          path: sampleWallpaper,
          name: "示例壁纸",
          source: "upload" as const,
          width: 1536,
          height: 1024,
          createdAt: "2026-09-12T00:00:00Z",
        }
      : null,
    blur: wallpaperBlur,
    followSystemWallpaper: false,
  };
  const presentation = resolveWallpaperPresentation(null, wallpaperPrefs);
  return (
    <main
      className={`app-shell soft-material-sample-stage${wallpaper ? " has-wallpaper" : ""}`}
      data-tone="amber"
    >
      {wallpaper ? (
        <div
          className="shell-wallpaper-layer"
          aria-hidden
          style={
            {
              "--wallpaper-blur": presentation.blur,
              "--wallpaper-shade": presentation.shade / 100,
            } as CSSProperties
          }
        >
          <img src={sampleWallpaper} alt="" style={{ objectFit: "cover" }} />
          <span />
        </div>
      ) : null}
      <div className="soft-material-sample">
        <section
          className="soft-material-sample-chat"
          aria-label="聊天材质样板"
        >
          <header>
            <strong>Astro</strong>
            <span>对话 · 乳白磨砂材质样板</span>
            <button
              type="button"
              className="ui-button ui-button--secondary ui-button--sm"
              aria-pressed={wallpaper}
              onClick={() => setWallpaper(!wallpaper)}
            >
              示例壁纸
            </button>
          </header>
          <article className="bubble user">
            帮我整理一下今天的工作计划。
          </article>
          <article className="bubble assistant">
            <ChatMarkdown
              content={
                "### 让注意力回到内容\n\n柔和的背景色透过磨砂表面，文字始终清晰。\n\n- 卡片细白边，长文保持安静\n- 输入区域乳白透亮，按钮更厚实\n- 明暗独立切换，保留你的强调色\n\n```typescript\nconst material = 'soft';\n```"
              }
            />
          </article>
          <div className="composer composer--stacked">
            <textarea
              className="composer-input"
              aria-label="样板输入框"
              placeholder="继续聊聊你的想法…"
            />
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <button type="button" className="composer-mode-pill">
                默认模式
              </button>
              <button
                type="button"
                className="send-btn send-btn--round"
                aria-label="样板发送按钮"
              >
                ↑
              </button>
            </div>
          </div>
        </section>
        <div className="settings-content-inline soft-material-sample-settings">
          <PreferencesPanel
            {...preferencesMeta.args}
            mode={mode}
            onChange={setMode}
            wallpaper={{
              ...preferencesMeta.args.wallpaper,
              prefs: wallpaperPrefs,
              setMode: (mode) => setWallpaper(mode === "wallpaper"),
              setBlur: setWallpaperBlur,
            }}
          />
        </div>
      </div>
    </main>
  );
}

const meta = {
  title: "Design/Soft Material",
  component: SoftMaterialSample,
  decorators: [
    (Story) => (
      <ThemeProvider>
        <LocaleProvider>
          <MorphiconProvider>
            <Story />
          </MorphiconProvider>
        </LocaleProvider>
      </ThemeProvider>
    ),
  ],
  beforeEach: () => {
    mockIPC((command) => {
      if (command === "get_app_icon") return { current: "blue", options: [] };
      // Off-screen context settings are not part of this material fixture.
      // An empty array is not a CompressionSettingsDto (it creates NaN inputs).
      if (command === "get_compression_settings") return null;
      if (command === "plugin:event|listen") return 1;
      if (command === "plugin:event|unlisten") return null;
      return [];
    });
    return clearMocks;
  },
} satisfies Meta<typeof SoftMaterialSample>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
