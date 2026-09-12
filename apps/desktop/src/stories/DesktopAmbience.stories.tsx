import { useState } from "react";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import DesktopAmbienceButton from "../components/ui/DesktopAmbienceButton";
import { LocaleProvider } from "../i18n/LocaleContext";
import { useShellColorStyle } from "../hooks/app/useShellColorStyle";
import { useWallpaper } from "../hooks/app/useWallpaper";
import { useActiveUiStyle } from "../hooks/app/useActiveUiStyle";
import { ambienceSession } from "../lib/ui/ambienceSession";
import {
  DEFAULT_WALLPAPER_PREFS,
  WALLPAPER_STORAGE_KEY,
} from "../lib/ui/wallpaper";
import {
  EMPTY_DESKTOP_PET_STATE,
  type DesktopPetState,
} from "../lib/ui/desktopPetState";
import type { PetScene } from "../lib/ui/petScene";
import type { ActiveUiStyle } from "../lib/ui/activeUiStyle";
import room from "./assets/ambience-window.jpeg";
import forest from "./assets/ambience-forest.jpeg";
import meadow from "./assets/ambience-meadow.jpeg";
import atlas from "../assets/pets/naitang/spritesheet.webp";

function Harness() {
  const [managed, setManaged] = useState("");
  const wallpaper = useWallpaper();
  const colors = useShellColorStyle();
  const activeStyle = useActiveUiStyle();
  return (
    <LocaleProvider>
      <main
        style={{
          position: "relative",
          height: "100vh",
          background: "var(--bg)",
          color: "var(--ink)",
          padding: 28,
        }}
      >
        <h2>桌面氛围</h2>
        <p>测试环境 · 所有操作均为本地模拟，不调用模型。</p>
        <output data-testid="manage-target">{managed}</output>
        {managed ? (
          <button type="button" onClick={() => setManaged("")}>
            返回对话
          </button>
        ) : (
          <DesktopAmbienceButton
            wallpaper={wallpaper}
            colors={{
              style: colors.colorStyle,
              gradient: colors.gradient,
              dynamicSeed: colors.dynamicSeed,
            }}
            restoreColors={colors.restoreColorPrefs}
            activeStyle={activeStyle}
            theme={
              document.documentElement.dataset.theme === "dark"
                ? "dark"
                : "light"
            }
            onManage={setManaged}
          />
        )}
      </main>
    </LocaleProvider>
  );
}

const meta = {
  title: "Shell/DesktopAmbience",
  component: Harness,
  beforeEach: (context) => {
    ambienceSession.setUndo(null);
    ambienceSession.finish();
    localStorage.setItem("astro-locale", "zh");
    localStorage.setItem(
      WALLPAPER_STORAGE_KEY,
      JSON.stringify({
        ...DEFAULT_WALLPAPER_PREFS,
        followSystemWallpaper: false,
      }),
    );
    const url = (asset: string) => new URL(asset, window.location.href).href;
    let scenes: PetScene[] = [room, forest, meadow].map((asset, i) => {
      const id = `pet-scene-${i}`;
      const style: ActiveUiStyle = {
        schemaVersion: 1,
        id,
        name: ["午后窗台", "森林小屋", "晴日草地"][i],
        revision: id,
        updatedAt: "now",
        icons: {},
        tokens: { light: {}, dark: {} },
        wallpaper: {
          path: url(asset),
          fit: "cover",
          shade: 18,
          blur: 0,
          adaptiveColor: true,
          accentColor: "#47775b",
          secondaryColor: "#956535",
          recommendedTheme: "light",
        },
      };
      return {
        id,
        name: style.name,
        pet: {
          ...EMPTY_DESKTOP_PET_STATE,
          petId: i < 2 ? "naitang" : "pudding",
          petPath: url(atlas),
          displayName: i < 2 ? "奶糖" : "布丁",
        },
        style,
        wallpaperPath: url(asset),
        inUse: i === 0,
        favorite: false,
      };
    });
    if (context.name === "Empty") scenes = [];
    let state: DesktopPetState = {
      ...EMPTY_DESKTOP_PET_STATE,
      activePetId: "naitang",
      petPath: url(atlas),
      displayName: "奶糖",
      activeSceneId: scenes[0]?.id ?? null,
      scenes,
      revision: 1,
    };
    let style: ActiveUiStyle | null = scenes[0]?.style ?? null;
    let undo: { state: typeof state; style: ActiveUiStyle | null } | null =
      null;
    mockIPC(async (command, payload) => {
      const args = payload as Record<string, any>;
      if (command === "plugin:event|listen") return 1;
      if (
        command === "plugin:event|unlisten" ||
        command === "set_app_menu_locale"
      )
        return null;
      if (
        command === "get_desktop_pet_state" ||
        command === "sync_pet_scene_wallpaper"
      )
        return structuredClone(state);
      if (command === "get_pet_scenes") return structuredClone(scenes);
      if (command === "get_active_ui_style") return structuredClone(style);
      if (command === "apply_desktop_ambience") {
        if (context.name === "Failure")
          throw new Error("测试：切换失败，原外观未改变");
        undo = structuredClone({ state, style });
        if (args.change.kind === "scene") {
          const scene = scenes.find((s) => s.id === args.change.sceneId)!;
          style = { ...scene.style!, revision: `${state.revision + 1}` };
          state = {
            ...state,
            activeSceneId: scene.id,
            activePetId: scene.pet.petId,
            displayName: scene.pet.displayName!,
            revision: state.revision + 1,
          };
        } else {
          if (args.change.kind === "palette" && style?.wallpaper) {
            const colors = args.change.colors;
            const tokens: Record<string, string> = colors
              ? {
                  "--color-accent": colors[0],
                  "--color-accent-secondary": colors[1],
                }
              : {};
            style = {
              ...style,
              id: `ambience-${state.revision + 1}`,
              revision: `${state.revision + 1}`,
              tokens: { light: tokens, dark: tokens },
              wallpaper: {
                ...style.wallpaper,
                adaptiveColor: args.change.adaptiveColor,
              },
            };
          } else style = null;
          state = {
            ...state,
            activeSceneId:
              args.change.kind === "palette" ? state.activeSceneId : null,
            revision: state.revision + 1,
          };
        }
        return { state: structuredClone(state), undoToken: "fixture-undo" };
      }
      if (command === "undo_desktop_ambience" && undo) {
        const revision = state.revision + 1;
        state = { ...undo.state, revision };
        style = undo.style;
        undo = null;
        return structuredClone(state);
      }
      if (command === "save_desktop_ambience_scene") {
        const newScene = {
          ...scenes[0],
          id: `pet-saved-${scenes.length}`,
          name: args.name,
        };
        scenes = [...scenes, newScene];
        state = { ...state, scenes, revision: state.revision + 1 };
        return structuredClone(state);
      }
      if (command === "analyze_wallpaper")
        return {
          luminance: 0.5,
          recommendedTheme: "light",
          accentColor: "#47775b",
          secondaryColor: "#956535",
        };
      if (command === "get_system_wallpaper")
        return {
          id: "system",
          path: url(meadow),
          name: "系统壁纸",
          source: "system",
          width: 1536,
          height: 1024,
          createdAt: "now",
        };
      throw new Error(`Unexpected command: ${command}`);
    });
    return () => {
      clearMocks();
    };
  },
  render: () => <Harness />,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof Harness>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {};
export const Empty: Story = {};
export const Failure: Story = {};
