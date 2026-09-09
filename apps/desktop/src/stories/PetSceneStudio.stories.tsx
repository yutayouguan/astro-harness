import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { Meta, StoryObj } from "@storybook/react-vite";
import DesktopPetPanel from "../components/settings/DesktopPetPanel";
import { LocaleProvider } from "../i18n/LocaleContext";
import { EMPTY_DESKTOP_PET_STATE } from "../lib/ui/desktopPetState";
import concept from "../assets/generated/desktop-pet-concept.png";

const meta = {
  title: "Settings/PetSceneStudio",
  component: DesktopPetPanel,
  beforeEach: () => {
    const photo = new URL(concept, window.location.href).href;
    let state = {
      ...EMPTY_DESKTOP_PET_STATE,
      revision: 1,
      sourcePath: photo,
      petPath: photo,
    };
    const pet = {
      petPath: photo,
      sourcePath: photo,
      spriteVersionNumber: null,
      displayName: "奶糖",
      description: null,
      provider: "Fixture",
      model: "No API calls",
    };
    let scenes = [
      {
        id: "pet-demo",
        name: "奶糖 · 场景预览样例",
        pet,
        style: null,
        wallpaperPath: photo,
        favorite: true,
        inUse: true,
      },
      {
        id: "pet-draft",
        name: "桌宠已保存 · 待配壁纸",
        pet,
        style: null,
        wallpaperPath: null,
        favorite: false,
        inUse: false,
      },
    ];
    mockIPC((command, payload) => {
      const args = payload as Record<string, unknown>;
      if (command === "get_pet_scenes") return scenes;
      if (command === "plugin:event|listen") return 1;
      if (command === "plugin:event|unlisten") return null;
      if (command === "get_desktop_pet_state") return state;
      if (command === "cancel_pet_generation") return true;
      if (command === "edit_pet_scene") {
        const edit = args.request as {
          action: string;
          sceneId: string;
          name: string;
          favorite: boolean;
        };
        const target = scenes.find((s) => s.id === edit.sceneId);
        if (edit.action === "rename" && target) target.name = edit.name;
        if (edit.action === "rename_pet" && target)
          scenes.forEach((s) => {
            if (s.pet.petPath === target.pet.petPath)
              s.pet.displayName = edit.name;
          });
        if (edit.action === "favorite" && target)
          target.favorite = edit.favorite;
        if (edit.action === "duplicate" && target)
          scenes.push({
            ...target,
            id: `pet-home-${state.revision}`,
            name: edit.name,
            wallpaperPath: null,
            favorite: false,
            inUse: false,
          });
        if (edit.action === "delete")
          scenes = scenes.filter((s) => s.id !== edit.sceneId);
      }
      if (command === "generate_pet_scene_wallpaper")
        throw new Error("验收样例：壁纸请求失败，已保留桌宠，可单独重试。");
      if (command === "set_pet_scene_follow_wallpaper")
        state = { ...state, followWallpaper: Boolean(args.enabled) };
      if (command === "apply_pet_scene")
        state = {
          ...state,
          enabled: args.mode !== "wallpaper",
          followWallpaper: args.mode === "all",
        };
      if (command === "create_pet_scene") {
        const scene = {
          id: `pet-fixture-${state.revision}`,
          name: String(args.name),
          pet,
          style: null,
          wallpaperPath: null,
          favorite: false,
          inUse: false,
        };
        scenes.unshift(scene);
        state = {
          ...state,
          scenes: [...state.scenes, { id: scene.id, name: scene.name }],
        };
      }
      state = { ...state, revision: state.revision + 1 };
      return state;
    });
    return clearMocks;
  },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <main
          style={{
            maxWidth: 1060,
            margin: "auto",
            padding: 24,
            color: "var(--ink)",
            background: "var(--shell-bg)",
          }}
        >
          <p role="note">
            交互验收样例：复用概念图，不调用模型、不修改真实桌宠。
          </p>
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
