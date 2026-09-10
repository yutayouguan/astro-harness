import { Eye, EyeOff, PawPrint, Plus, Settings2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { useDesktopPetState } from "../../hooks/app/useDesktopPetState";
import { useI18n } from "../../i18n/LocaleContext";
import { DEFAULT_PET_PREFERENCES } from "../../lib/ui/petPreferences";
import { SegmentedTabs } from "../ui/SegmentedTabs";
import conceptImage from "../../assets/generated/desktop-pet-concept.png";
import PetCreatePanel from "./PetCreatePanel";
import PetLibraryPanel from "./PetLibraryPanel";
export type { DesktopPetState } from "../../lib/ui/desktopPetState";

export default function DesktopPetPanel({ active }: { active: boolean }) {
  const { locale } = useI18n(),
    zh = locale === "zh";
  const { state, loading, pending, error, mutate } = useDesktopPetState(active);
  const [tab, setTab] = useState("library");
  const [visible, setVisible] = useState<boolean | null>(null);
  const [localError, setLocalError] = useState("");
  const busy = loading || pending > 0;
  const preferences = state.preferences ?? DEFAULT_PET_PREFERENCES;
  useEffect(() => {
    if (!active) return;
    let disposed = false,
      received = false;
    let stop: (() => void) | undefined;
    void listen<boolean>("desktop-pet-visibility", ({ payload }) => {
      received = true;
      if (!disposed) setVisible(payload);
    })
      .then(async (cleanup) => {
        if (disposed) {
          cleanup();
          return;
        }
        stop = cleanup;
        const value = await invoke<boolean>("get_desktop_pet_visible");
        if (!disposed && !received) setVisible(value);
      })
      .catch(() => {});
    return () => {
      disposed = true;
      stop?.();
    };
  }, [active]);
  async function run(command: string, args: Record<string, unknown> = {}) {
    setLocalError("");
    try {
      await mutate(command, args);
    } catch (e) {
      setLocalError(String(e));
    }
  }
  const sceneName = state.scenes.find(
    (s) => s.id === state.activeSceneId,
  )?.name;
  const status = !state.enabled
    ? zh
      ? "已关闭"
      : "Disabled"
    : visible === false
      ? zh
        ? "暂时隐藏"
        : "Temporarily hidden"
      : visible === true
        ? zh
          ? "显示中"
          : "Visible"
        : zh
          ? "状态同步中"
          : "Syncing";
  return (
    <section className="desktop-pet-settings pet-manager" aria-busy={busy}>
      <header className="desktop-pet-hero">
        <div className="desktop-pet-hero-copy">
          <span className="desktop-pet-eyebrow">ASTRO DESKTOP COMPANION</span>
          <h2>
            {zh
              ? "把熟悉的它，带到桌面上"
              : "Bring a familiar friend to your desktop"}
          </h2>
          <p>
            {zh
              ? "收藏你的桌面伙伴，为它准备不同的场景。从一张照片开始，让每一次陪伴都有自己的模样。"
              : "Collect your companions and give each one a home. Start with a photo and make every moment together your own."}
          </p>
        </div>
        <img src={conceptImage} alt="" aria-hidden />
      </header>
      <div className="pet-manager-commandbar">
        <SegmentedTabs
          className="pet-manager-tabs"
          aria-label={zh ? "桌宠管理" : "Pet manager"}
          value={tab}
          onValueChange={setTab}
          items={[
            {
              value: "library",
              label: zh ? "宠物库" : "Library",
              icon: <PawPrint size={16} />,
              count: state.pets?.length ?? 0,
              panelId: "pet-library-panel",
            },
            {
              value: "create",
              label: zh ? "创建宠物" : "Create",
              icon: <Plus size={16} />,
              panelId: "pet-create-panel",
            },
            {
              value: "general",
              label: zh ? "通用设置" : "General",
              icon: <Settings2 size={16} />,
              panelId: "pet-general-panel",
            },
          ]}
        />
        <div className="pet-manager-current">
          <span className="desktop-pet-icon">
            <PawPrint size={20} />
          </span>
          <div>
            <strong>
              {zh ? "当前桌面" : "On your desktop"} ·{" "}
              {state.displayName || (zh ? "尚未选择宠物" : "No pet selected")}
            </strong>
            <small>
              {sceneName ||
                (zh
                  ? "默认陪伴 · 不更换壁纸"
                  : "Default companion · wallpaper unchanged")}{" "}
              · {status}
            </small>
          </div>
          <button
            type="button"
            className="desktop-pet-import-package"
            disabled={busy || !state.petPath}
            onClick={() =>
              void run("set_desktop_pet_enabled", { enabled: !state.enabled })
            }
          >
            {state.enabled ? <EyeOff size={16} /> : <Eye size={16} />}
            {state.enabled ? (zh ? "隐藏" : "Hide") : zh ? "显示" : "Show"}
          </button>
        </div>
      </div>
      {(error || localError) && (
        <p className="desktop-pet-error" role="alert">
          {localError || error}
        </p>
      )}
      <div
        role="tabpanel"
        aria-label={zh ? "宠物库" : "Library"}
        id="pet-library-panel"
        hidden={tab !== "library"}
      >
        <PetLibraryPanel
          state={state}
          mutate={mutate}
          busy={busy}
          zh={zh}
          active={active && tab === "library"}
          onCreate={() => setTab("create")}
        />
      </div>
      <div
        role="tabpanel"
        aria-label={zh ? "创建宠物" : "Create"}
        id="pet-create-panel"
        hidden={tab !== "create"}
      >
        <PetCreatePanel
          state={state}
          mutate={mutate}
          pending={busy}
          locale={locale}
        />
      </div>
      <div
        role="tabpanel"
        aria-label={zh ? "通用设置" : "General"}
        id="pet-general-panel"
        hidden={tab !== "general"}
      >
        <section className="prefs-card desktop-pet-card pet-general-card">
          <header className="desktop-pet-card-head">
            <div>
              <h3>
                {zh ? "所有宠物共用的桌面策略" : "Desktop-wide preferences"}
              </h3>
              <p>
                {zh
                  ? "切换宠物或场景不会覆盖以下显示策略。"
                  : "Switching companions never overrides these visibility policies."}
              </p>
            </div>
          </header>
          <div className="pet-general-options">
            {(
              [
                [
                  "alwaysOnTop",
                  zh ? "始终置顶" : "Always on top",
                  state.alwaysOnTop,
                  "set_desktop_pet_always_on_top",
                  { alwaysOnTop: !state.alwaysOnTop },
                ],
                [
                  "followWallpaper",
                  zh ? "随配套壁纸切换宠物" : "Follow paired wallpapers",
                  state.followWallpaper,
                  "set_pet_scene_follow_wallpaper",
                  { enabled: !state.followWallpaper },
                ],
                [
                  "hideInFullscreen",
                  zh ? "全屏时自动隐藏" : "Hide in fullscreen",
                  preferences.hideInFullscreen,
                  "configure_desktop_pet_preferences",
                  {
                    patch: { hideInFullscreen: !preferences.hideInFullscreen },
                  },
                ],
                [
                  "presentationMode",
                  zh ? "演示时暂时隐藏" : "Presentation hide",
                  preferences.presentationMode,
                  "configure_desktop_pet_preferences",
                  {
                    patch: { presentationMode: !preferences.presentationMode },
                  },
                ],
              ] as const
            ).map(([key, label, checked, command, args]) => (
              <label key={key} className="desktop-pet-preference-row">
                <span>{label}</span>
                <input
                  type="checkbox"
                  checked={checked}
                  disabled={busy}
                  onChange={() => void run(command, args)}
                />
              </label>
            ))}
          </div>
          <p className="desktop-pet-model">
            {zh
              ? "未关联的壁纸保留当前宠物；仅应用宠物会关闭壁纸联动。全屏检测在 macOS 支持前台应用。"
              : "Unpaired wallpapers keep your pet. Pet-only applies disable linking. Foreground fullscreen detection is available on macOS."}
          </p>
          <div className="pet-scene-actions">
            <button
              type="button"
              disabled={busy || !state.petPath}
              onClick={() => void run("reset_desktop_pet_position")}
            >
              {zh ? "回到屏幕内" : "Bring back on screen"}
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void run("resume_desktop_pet")}
            >
              {zh ? "恢复显示" : "Restore visibility"}
            </button>
          </div>
        </section>
      </div>
    </section>
  );
}
