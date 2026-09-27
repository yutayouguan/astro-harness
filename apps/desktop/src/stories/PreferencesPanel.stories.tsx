import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import PreferencesPanel from "../components/settings/PreferencesPanel";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { ThemeProvider } from "../hooks/app/useTheme";
import { LocaleProvider } from "../i18n/LocaleContext";
import { DEFAULT_SHELL_GRADIENT } from "../lib/ui/shellGradient";
import { DEFAULT_WALLPAPER_PREFS } from "../lib/ui/wallpaper";

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
    wallpaper: {
      replacePrefs: () => {},
      withSuspendedSync: async (operation) => operation(),
      prefs: DEFAULT_WALLPAPER_PREFS,
      busy: null,
      error: null,
      setMode: () => {},
      setFit: () => {},
      setShade: () => {},
      setBlur: () => {},
      setAdaptiveColor: () => {},
      setPalette: () => {},
      setFollowSystemWallpaper: () => {},
      select: () => {},
      cycleRecent: () => {},
      importImage: async () => {
        throw new Error("not available in Storybook");
      },
      generate: async () => {
        throw new Error("not available in Storybook");
      },
      pending: null,
      previous: null,
      applyPending: () => null,
      discardPending: () => {},
      undoApply: () => false,
      cancelGeneration: async () => false,
      clearError: () => {},
      markCurrentUnavailable: () => {},
    },
    tone: "twilight",
    chatDisplayPrefs: {
      verbosity: "normal",
      answerLayout: "timeline",
      showTools: true,
      showSkills: true,
      showMcp: true,
      showHooks: false,
      showMemory: true,
      showStatus: true,
      showTimestamps: false,
      processDefaultOpen: false,
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

export const Conversation: Story = {
  args: {
    section: "conversation",
  },
};

export const Wallpaper: Story = {
  args: {
    wallpaper: {
      ...meta.args.wallpaper,
      prefs: {
        ...DEFAULT_WALLPAPER_PREFS,
        mode: "wallpaper",
      },
    },
  },
};
