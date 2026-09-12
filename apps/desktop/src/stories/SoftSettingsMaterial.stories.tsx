import { useState } from "react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import DesktopPetPanel from "../components/settings/DesktopPetPanel";
import EnvironmentDependenciesPanel from "../components/settings/EnvironmentDependenciesPanel";
import PreferencesPanel from "../components/settings/PreferencesPanel";
import { ThemeProvider, useTheme } from "../hooks/app/useTheme";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { EMPTY_DESKTOP_PET_STATE } from "../lib/ui/desktopPetState";
import { DEFAULT_PET_PREFERENCES } from "../lib/ui/petPreferences";
import preferencesMeta from "./PreferencesPanel.stories";
import { DEFAULT_SHELL_GRADIENT } from "../lib/ui/shellGradient";
import portrait from "../assets/generated/desktop-pet-concept.png";

function MaterialSettingsSample() {
  const {
    mode,
    setMode,
    material,
    setMaterial,
    softFrostIntensity,
    setSoftFrostIntensity,
  } = useTheme();
  const [page, setPage] = useState("dependencies");
  const [backgroundMode, setBackgroundMode] = useState<"color" | "wallpaper">(
    "color",
  );
  const [colorStyle, setColorStyle] = useState<
    "unified" | "dynamic" | "colorful"
  >("unified");
  const [gradient, setGradient] = useState(DEFAULT_SHELL_GRADIENT);
  return (
    <main
      className="settings-content-inline"
      style={{
        height: "100vh",
        overflow: "auto",
        padding: 24,
        boxSizing: "border-box",
        background:
          "repeating-linear-gradient(135deg, var(--soft-base, #eceef2) 0 80px, var(--soft-inset, #dfe3ec) 80px 160px)",
        color: "var(--ink)",
      }}
    >
      <nav
        style={{ display: "flex", flexWrap: "wrap", gap: 12, marginBottom: 20 }}
      >
        <button
          onClick={() => setMaterial(material === "soft" ? "glass" : "soft")}
        >
          材质：{material}
        </button>
        <button onClick={() => setMode(mode === "dark" ? "light" : "dark")}>
          切换明暗
        </button>
        <label>
          毛玻璃强度
          <input
            aria-label="样板毛玻璃强度"
            type="range"
            min={0}
            max={100}
            value={softFrostIntensity}
            onChange={(e) =>
              setSoftFrostIntensity(Number(e.currentTarget.value))
            }
          />
        </label>
        <output>{softFrostIntensity}%</output>
        <button onClick={() => setPage("appearance")}>外观设置</button>
        <button onClick={() => setPage("dependencies")}>环境依赖</button>
        <button onClick={() => setPage("pets")}>桌面宠物</button>
      </nav>
      <p role="note">
        隔离样板：只读模拟数据，不安装依赖、不调用模型、不修改真实宠物。
      </p>
      {page === "appearance" ? (
        <PreferencesPanel
          {...preferencesMeta.args}
          mode={mode}
          onChange={setMode}
          colorStyle={colorStyle}
          onColorStyleChange={setColorStyle}
          gradient={gradient}
          onGradientChange={setGradient}
          wallpaper={{
            ...preferencesMeta.args.wallpaper,
            prefs: {
              ...preferencesMeta.args.wallpaper.prefs,
              mode: backgroundMode,
            },
            setMode: setBackgroundMode,
          }}
        />
      ) : page === "pets" ? (
        <DesktopPetPanel active />
      ) : (
        <EnvironmentDependenciesPanel />
      )}
    </main>
  );
}

const meta = {
  title: "Design/Soft Settings Material",
  component: MaterialSettingsSample,
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
    const identity = {
      petId: "frost-preview",
      displayName: "奶糖",
      petPath: new URL(portrait, location.href).href,
      sourcePath: null,
      groomingPath: null,
      spriteVersionNumber: null,
      description: "只读材质样例",
      provider: null,
      model: null,
    };
    mockIPC((command) => {
      if (command === "plugin:event|listen") return 1;
      if (command === "plugin:event|unlisten") return null;
      if (command === "get_app_icon") return { current: "blue", options: [] };
      if (command === "get_desktop_pet_state")
        return {
          ...EMPTY_DESKTOP_PET_STATE,
          ...identity,
          activePetId: identity.petId,
          enabled: true,
          pets: [
            {
              id: identity.petId,
              builtin: true,
              identity,
              defaults: { scale: 0.4, behavior: DEFAULT_PET_PREFERENCES },
            },
          ],
        };
      if (command === "get_desktop_pet_visible") return true;
      if (command === "get_pet_scenes") return [];
      if (command === "list_environment_dependencies")
        return ["uv", "rtk", "fd", "ripgrep", "bun", "lark-cli"].map(
          (id, i) => ({
            id,
            name: id,
            binary: id,
            installed: i !== 5,
            version: i !== 5 ? "1.0.0" : null,
            path: i !== 5 ? `/preview/bin/${id}` : null,
            installPath: `/preview/bin/${id}`,
            installCommand: `brew install ${id}`,
            canInstall: true,
            installUnavailableReason: null,
          }),
        );
      throw new Error(`Read-only material preview: ${command} is disabled`);
    });
    return clearMocks;
  },
} satisfies Meta<typeof MaterialSettingsSample>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
