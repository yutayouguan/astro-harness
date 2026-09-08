import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { Images, PawPrint, RefreshCw, Upload } from "lucide-react";
import { useReducedMotion } from "framer-motion";
import DesktopPetCanvas from "../desktop-pet/DesktopPetCanvas";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import type { PetScene } from "../../lib/ui/petScene";
import type { DesktopPetState } from "../../lib/ui/desktopPetState";
import {
  normalizeWallpaperPrefs,
  WALLPAPER_STORAGE_KEY,
  type WallpaperAsset,
} from "../../lib/ui/wallpaper";

export default function PetSceneLibrary({
  state,
  mutate,
  busy,
  zh,
  name,
  description,
  includePet,
}: {
  state: DesktopPetState;
  mutate: (
    command: string,
    args: Record<string, unknown>,
  ) => Promise<DesktopPetState>;
  busy: boolean;
  zh: boolean;
  name: string;
  description: string;
  includePet: boolean;
}) {
  const [scenes, setScenes] = useState<PetScene[]>([]);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const [recent, setRecent] = useState<WallpaperAsset[]>([]);
  const reduced = useReducedMotion();
  const disabled = busy || working;
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let cancelled = false;
    void invoke<PetScene[]>("get_pet_scenes")
      .then((next) => {
        if (!cancelled) setScenes(next);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [state.revision]);

  async function run(operation: () => Promise<unknown>) {
    if (disabled) return;
    setWorking(true);
    setError("");
    try {
      await operation();
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
    }
  }

  async function bind(sceneId: string) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({
      multiple: false,
      directory: false,
      filters: [
        {
          name: "Wallpaper",
          extensions: ["png", "jpg", "jpeg", "webp", "bmp"],
        },
      ],
    });
    if (typeof path === "string")
      await mutate("bind_pet_scene_wallpaper", { sceneId, sourcePath: path });
  }

  function loadRecent() {
    try {
      setRecent(
        normalizeWallpaperPrefs(
          JSON.parse(localStorage.getItem(WALLPAPER_STORAGE_KEY) || "null"),
        ).recent,
      );
    } catch {
      setRecent([]);
    }
  }

  return (
    <section className="prefs-card pet-scene-library" aria-busy={disabled}>
      <header className="desktop-pet-card-head">
        <Images size={20} aria-hidden />
        <div>
          <h3>{zh ? "宠物场景收藏" : "Companion scenes"}</h3>
          <p>
            {zh
              ? "先预览，再应用。切换收藏不消耗生成额度。"
              : "Preview before applying. Switching saved scenes never generates images."}
          </p>
        </div>
      </header>
      <label className="desktop-pet-toggle-row">
        <span>
          <strong>{zh ? "随壁纸切换桌宠" : "Follow wallpaper"}</strong>
          <small>
            {zh
              ? "未绑定的壁纸保留当前桌宠；仅桌宠操作会关闭联动。"
              : "Unpaired wallpapers keep your pet. Pet-only applies disable linking."}
          </small>
        </span>
        <input
          type="checkbox"
          checked={state.followWallpaper}
          disabled={disabled}
          onChange={(e) =>
            void run(() =>
              mutate("set_pet_scene_follow_wallpaper", {
                enabled: e.currentTarget.checked,
              }),
            )
          }
        />
      </label>
      <button
        type="button"
        className="desktop-pet-import-package"
        disabled={disabled || !state.petPath}
        onClick={() =>
          void run(() =>
            mutate("create_pet_scene", {
              name: name.trim() || (zh ? "我的宠物场景" : "My companion scene"),
              description: null,
              useCurrent: true,
            }),
          )
        }
      >
        <PawPrint size={16} />
        {zh ? "收藏当前桌宠 / 添加新场景" : "Save current pet / add scene"}
      </button>
      {working && (
        <p role="status">
          {zh ? "正在处理场景，请稍候…" : "Updating scene, please wait…"}
        </p>
      )}
      {error && (
        <p className="desktop-pet-error" role="alert">
          {error}
        </p>
      )}
      <div className="pet-scene-list">
        {scenes.map((scene) => (
          <article key={scene.id} className="pet-scene-card">
            <div className="pet-scene-preview">
              {scene.wallpaperPath && (
                <img
                  className="pet-scene-wallpaper"
                  src={resolveMediaSrc(scene.wallpaperPath) || undefined}
                  alt=""
                />
              )}
              {scene.pet.spriteVersionNumber === 2 ? (
                <DesktopPetCanvas
                  src={resolveMediaSrc(scene.pet.petPath) || ""}
                  state="idle"
                  className="pet-scene-pet"
                  label={scene.name}
                  reducedMotion={Boolean(reduced)}
                />
              ) : (
                <img
                  className="pet-scene-pet"
                  src={resolveMediaSrc(scene.pet.petPath) || undefined}
                  alt={scene.name}
                />
              )}
            </div>
            <h4>{scene.name}</h4>
            <p>
              {scene.wallpaperPath
                ? zh
                  ? "桌宠 + 配套壁纸"
                  : "Pet + wallpaper"
                : zh
                  ? "桌宠已保存，可补生成或关联壁纸"
                  : "Pet saved. Generate or attach a wallpaper."}
            </p>
            <div className="pet-scene-actions">
              {(["all", "pet", "wallpaper"] as const).map((mode) => (
                <button
                  key={mode}
                  type="button"
                  disabled={
                    disabled || (mode !== "pet" && !scene.wallpaperPath)
                  }
                  onClick={() =>
                    void run(() =>
                      mutate("apply_pet_scene", { sceneId: scene.id, mode }),
                    )
                  }
                >
                  {zh
                    ? { all: "应用整套", pet: "仅桌宠", wallpaper: "仅壁纸" }[
                        mode
                      ]
                    : {
                        all: "Apply both",
                        pet: "Pet only",
                        wallpaper: "Wallpaper only",
                      }[mode]}
                </button>
              ))}
            </div>
            <div className="pet-scene-actions">
              <select
                aria-label={
                  zh ? "选择最近使用的壁纸" : "Choose recent wallpaper"
                }
                value=""
                disabled={disabled}
                onFocus={loadRecent}
                onChange={(e) => {
                  const sourcePath = e.currentTarget.value;
                  if (sourcePath)
                    void run(() =>
                      mutate("bind_pet_scene_wallpaper", {
                        sceneId: scene.id,
                        sourcePath,
                      }),
                    );
                }}
              >
                <option value="">
                  {zh ? "最近使用的壁纸…" : "Recent wallpapers…"}
                </option>
                {recent.map((asset) => (
                  <option key={asset.id} value={asset.path}>
                    {asset.name}
                  </option>
                ))}
              </select>
              <button
                type="button"
                disabled={disabled}
                onClick={() => void run(() => bind(scene.id))}
              >
                <Upload size={14} />
                {zh ? "关联已有壁纸" : "Attach wallpaper"}
              </button>
              <button
                type="button"
                disabled={disabled}
                onClick={() =>
                  void run(() =>
                    mutate("generate_pet_scene_wallpaper", {
                      sceneId: scene.id,
                      description:
                        description.trim() ||
                        (zh
                          ? "温暖安静的森林小屋"
                          : "A warm peaceful woodland home"),
                      includePet,
                    }),
                  )
                }
              >
                <RefreshCw size={14} />
                {zh ? "生成 / 重试壁纸" : "Generate / retry wallpaper"}
              </button>
            </div>
          </article>
        ))}
      </div>
      {!scenes.length && (
        <p>
          {zh
            ? "生成新桌宠或收藏当前桌宠后，场景将出现在这里。"
            : "Generate or save a pet to start your scene library."}
        </p>
      )}
    </section>
  );
}
