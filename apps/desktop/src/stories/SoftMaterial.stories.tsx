import type { Meta, StoryObj } from "@storybook/react-vite";
import PreferencesPanel from "../components/settings/PreferencesPanel";
import { ChatMarkdown } from "../components/chat/ChatMarkdown";
import { ThemeProvider, useTheme } from "../hooks/app/useTheme";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import preferencesMeta from "./PreferencesPanel.stories";
import "./soft-material-sample.css";

function SoftMaterialSample() {
  const { mode, setMode } = useTheme();
  return (
    <div className="soft-material-sample">
      <section className="soft-material-sample-chat" aria-label="聊天材质样板">
        <header>
          <strong>Astro</strong>
          <span>对话 · 柔塑材质样板</span>
        </header>
        <article className="bubble user">帮我整理一下今天的工作计划。</article>
        <article className="bubble assistant">
          <ChatMarkdown
            content={
              "### 让注意力回到内容\n\n柔和的光影勾勒层次，文字始终清晰。\n\n- 卡片轻浮起，长文保持安静\n- 输入区域浅凹陷，操作有触感\n- 明暗独立切换，保留你的强调色\n\n```typescript\nconst material = 'soft';\n```"
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
        />
      </div>
    </div>
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
            <main className="soft-material-sample-stage">
              <Story />
            </main>
          </MorphiconProvider>
        </LocaleProvider>
      </ThemeProvider>
    ),
  ],
  beforeEach: () => preferencesMeta.beforeEach(),
} satisfies Meta<typeof SoftMaterialSample>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
