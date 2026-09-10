import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import DesktopPetPanel from "../components/settings/DesktopPetPanel";
import { LocaleProvider } from "../i18n/LocaleContext";
import {
  EMPTY_DESKTOP_PET_STATE,
  type DesktopPetState,
} from "../lib/ui/desktopPetState";
import { DEFAULT_PET_PREFERENCES } from "../lib/ui/petPreferences";
import type { PetScene } from "../lib/ui/petScene";
import type { PetDefaults } from "../lib/ui/petLibrary";
import atlas from "../assets/pets/naitang/spritesheet.webp";
import concept from "../assets/generated/desktop-pet-concept.png";

const meta = {
  title: "Settings/PetSceneStudio",
  component: DesktopPetPanel,
  beforeEach: () => {
    const photo = new URL(concept, window.location.href).href;
    const identity = {
      petId: "fixture-naitang",
      petPath: new URL(atlas, window.location.href).href,
      groomingPath: null,
      sourcePath: null,
      spriteVersionNumber: 2,
      displayName: "奶糖",
      description: "内置橘白猫，管理预览不改变桌面。",
      provider: null,
      model: null,
    };
    const defaults = { scale: 0.4, behavior: { ...DEFAULT_PET_PREFERENCES } };
    let scenes: PetScene[] = [
      {
        id: "pet-forest",
        name: "森林小屋",
        pet: identity,
        style: null,
        wallpaperPath: photo,
        favorite: true,
        inUse: true,
        preferences: null,
      },
      {
        id: "pet-draft",
        name: "午后书房",
        pet: identity,
        style: null,
        wallpaperPath: null,
        favorite: false,
        inUse: false,
        preferences: null,
      },
    ];
    let state: DesktopPetState = {
      ...EMPTY_DESKTOP_PET_STATE,
      revision: 1,
      sourcePath: photo,
      petPath: identity.petPath,
      displayName: "奶糖",
      enabled: true,
      spriteVersionNumber: 2,
      activePetId: identity.petId,
      activeSceneId: "pet-forest",
      pets: [{ id: identity.petId, builtin: true, identity, defaults }],
      scenes,
    };
    mockIPC((command, payload) => {
      const args = payload as Record<string, unknown>;
      if (command === "plugin:event|listen") return 1;
      if (command === "plugin:event|unlisten") return null;
      if (command === "get_desktop_pet_state") return structuredClone(state);
      if (command === "get_desktop_pet_visible") return state.enabled;
      if (command === "get_pet_scenes") return structuredClone(scenes);
      if (command === "generate_pet_scene_wallpaper")
        throw new Error("验收样例：壁纸请求失败，已保留桌宠，可单独重试。");
      if (command === "cancel_pet_generation") return true;
      if (command === "set_desktop_pet_enabled") state.enabled = !!args.enabled;
      if (command === "set_pet_scene_follow_wallpaper")
        state.followWallpaper = !!args.enabled;
      if (command === "edit_pet_library") {
        const edit = args.request as {
          action: string;
          name: string;
          defaults: PetDefaults;
        };
        if (edit.action === "add_scene")
          scenes.push({
            ...scenes[0],
            id: "pet-" + state.revision,
            name: edit.name,
            wallpaperPath: null,
            preferences: null,
            favorite: false,
          });
        if (edit.action === "rename")
          state.pets![0].identity.displayName = edit.name;
        if (edit.action === "set_defaults")
          state.pets![0].defaults = edit.defaults;
      }
      if (command === "edit_pet_scene") {
        const edit = args.request as {
          action: string;
          sceneId: string;
          name: string;
          preferences: PetDefaults | null;
          favorite: boolean;
        };
        const scene = scenes.find((s) => s.id === edit.sceneId);
        if (edit.action === "rename" && scene) scene.name = edit.name;
        if (edit.action === "favorite" && scene) scene.favorite = edit.favorite;
        if (edit.action === "set_preferences" && scene)
          scene.preferences = edit.preferences;
        if (edit.action === "delete")
          scenes = scenes.filter((s) => s.id !== edit.sceneId);
      }
      if (command === "apply_library_pet") state.activeSceneId = null;
      if (command === "apply_pet_scene") {
        state.activeSceneId = String(args.sceneId);
        state.enabled = true;
      }
      state = { ...state, scenes, revision: state.revision + 1 };
      return structuredClone(state);
    });
    return clearMocks;
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <main
          className="settings-content-inline"
          style={{
            maxWidth: 1500,
            height: "100vh",
            boxSizing: "border-box",
            overflow: "auto",
            margin: "auto",
            padding: 24,
            color: "var(--ink)",
            background: "var(--shell-bg)",
          }}
        >
          <p role="note">隔离验收样例：不调用模型、不修改真实宠物。</p>
          <Story />
        </main>
      </LocaleProvider>
    ),
  ],
  args: { active: true },
} satisfies Meta<typeof DesktopPetPanel>;
export default meta;
type Story = StoryObj<typeof meta>;
export const PreviewAndFailure: Story = {};
