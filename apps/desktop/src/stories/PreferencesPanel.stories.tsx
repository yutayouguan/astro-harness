import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import PreferencesPanel from "../components/settings/PreferencesPanel";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { ThemeProvider } from "../hooks/app/useTheme";
import { LocaleProvider } from "../i18n/LocaleContext";
import { DEFAULT_SHELL_GRADIENT } from "../lib/ui/shellGradient";

const meta = {
  id: "preferences-panel",
  title: "Settings/PreferencesPanel",
  component: PreferencesPanel,
  beforeEach: () => {
    mockIPC((command) => {
      if (command === "get_app_icon") {
        return { current: "blue", options: [] };
      }
      return [];
    });
    return () => clearMocks();
  },
  decorators: [
    (Story) => (
      <ThemeProvider>
        <LocaleProvider>
          <MorphiconProvider>
            <main
              style={{
                boxSizing: "border-box",
                width: "100vw",
                height: "100vh",
                padding: 24,
                overflow: "hidden",
                background: "var(--shell-bg)",
                color: "var(--ink)",
              }}
            >
              <Story />
            </main>
          </MorphiconProvider>
        </LocaleProvider>
      </ThemeProvider>
    ),
  ],
  args: {
    mode: "light",
    onChange: () => {},
    colorStyle: "colorful",
    onColorStyleChange: () => {},
    gradient: DEFAULT_SHELL_GRADIENT,
    onGradientChange: () => {},
    onBeginCustomGradient: () => {},
    onPreviewGradient: () => {},
    onCommitCustomGradient: () => {},
    onCancelCustomGradient: () => {},
    onReshuffleDynamic: () => {},
    tone: "twilight",
    chatDisplayPrefs: {
      verbosity: "normal",
      answerLayout: "timeline",
      showTools: true,
      showSkills: true,
      showMcp: false,
      showHooks: true,
      showMemory: true,
      showStatus: true,
      showTimestamps: false,
    },
    onChatVerbosityChange: () => {},
    onChatAnswerLayoutChange: () => {},
    onChatToggleChange: () => {},
    section: "appearance",
  },
} satisfies Meta<typeof PreferencesPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Appearance: Story = {};
