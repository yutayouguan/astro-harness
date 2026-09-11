import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Star } from "lucide-react";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import { scenesForPet, type PetRecord } from "../../lib/ui/petLibrary";
import type { PetScene } from "../../lib/ui/petScene";
import {
  petScalePercent,
  type DesktopPetState,
} from "../../lib/ui/desktopPetState";
import {
  normalizeWallpaperPrefs,
  WALLPAPER_STORAGE_KEY,
  type WallpaperAsset,
} from "../../lib/ui/wallpaper";
import DesktopPetCanvas from "../desktop-pet/DesktopPetCanvas";
import PetPreferencesEditor from "./PetPreferencesEditor";
import PetMoreMenu from "./PetMoreMenu";

export default function PetSceneLibrary({
  state,
  mutate,
  busy,
  zh,
  pet,
  active,
  creating = false,
}: {
  state: DesktopPetState;
  mutate: (
    command: string,
    args: Record<string, unknown>,
  ) => Promise<DesktopPetState>;
  busy: boolean;
  zh: boolean;
  pet: PetRecord;
  active: boolean;
  creating?: boolean;
}) {
  const [scenes, setScenes] = useState<PetScene[]>([]);
  const [loading, setLoading] = useState(true);
  const loadedOnce = useRef(false);
  const [loadError, setLoadError] = useState("");
  const [working, setWorking] = useState(false),
    lock = useRef(false);
  const [error, setError] = useState(""),
    [notice, setNotice] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [name, setName] = useState(""),
    [description, setDescription] = useState("");
  const [includePet, setIncludePet] = useState(false);
  const [deleting, setDeleting] = useState<PetScene | null>(null);
  const [generation, setGeneration] = useState<string | null>(null);
  const [recent, setRecent] = useState<WallpaperAsset[]>([]);
  const disabled = busy || working;
  useEffect(() => {
    if (!active || !("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    // Revision updates refresh the data in place; only the initial read needs
    // a loading row. Inserting it again changes the list's scroll geometry.
    setLoading(!loadedOnce.current);
    setLoadError("");
    void invoke<PetScene[]>("get_pet_scenes")
      .then((next) => {
        if (!disposed) {
          loadedOnce.current = true;
          setScenes(next);
        }
      })
      .catch((e) => {
        if (!disposed) setLoadError(String(e));
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [state.revision, active]);
  async function run(operation: () => Promise<unknown>) {
    if (busy || lock.current) return;
    lock.current = true;
    setWorking(true);
    setError("");
    setNotice("");
    try {
      await operation();
    } catch (e) {
      setError(String(e));
    } finally {
      lock.current = false;
      setWorking(false);
    }
  }
  const edit = (request: Record<string, unknown>) =>
    mutate("edit_pet_scene", { request });
  async function bind(sceneId: string) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const sourcePath = await open({
      multiple: false,
      directory: false,
      filters: [
        {
          name: "Wallpaper",
          extensions: ["png", "jpg", "jpeg", "webp", "bmp"],
        },
      ],
    });
    if (typeof sourcePath === "string")
      await mutate("bind_pet_scene_wallpaper", { sceneId, sourcePath });
  }
  async function exportScene(sceneId: string) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const destination = await open({
      directory: true,
      multiple: false,
      title: zh ? "选择导出文件夹" : "Export folder",
    });
    if (typeof destination === "string") {
      const path = await invoke<string>("export_pet_scene", {
        sceneId,
        destination,
      });
      setNotice(
        (zh ? "已导出，不含原始照片：" : "Exported without source photo: ") +
          path,
      );
    }
  }
  async function generate(sceneId: string) {
    const requestId = crypto.randomUUID();
    setGeneration(requestId);
    try {
      await mutate("generate_pet_scene_wallpaper", {
        sceneId,
        requestId,
        description: description.trim(),
        includePet,
      });
    } finally {
      setGeneration(null);
    }
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
  const selected = scenes.find((s) => s.id === editing);
  return (
    <section className="pet-scenes" aria-busy={working}>
      {(error || loadError) && (
        <p role="alert" className="desktop-pet-error">
          {error || loadError}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
      {working && (
        <p role="status" className={generation ? undefined : "sr-only"}>
          {generation
            ? zh
              ? "正在生成壁纸…"
              : "Generating wallpaper…"
            : zh
              ? "正在保存…"
              : "Saving…"}
        </p>
      )}
      {generation && (
        <button
          type="button"
          onClick={() =>
            void invoke("cancel_pet_generation", { requestId: generation })
              .then(() =>
                setNotice(
                  zh
                    ? "已请求取消，已发送请求仍可能计费。"
                    : "Cancellation requested; submitted work may still be billed.",
                ),
              )
              .catch((e) => setError(String(e)))
          }
        >
          {zh ? "取消生成" : "Cancel generation"}
        </button>
      )}
      {deleting && (
        <div className="pet-scene-editor" role="alert">
          <p>
            {zh
              ? `移除「${deleting.name}」？只移除此场景，宠物、当前画面和素材文件保留。`
              : "Remove only this scene? The pet, active visuals and files are retained."}
          </p>
          <button
            type="button"
            disabled={disabled}
            onClick={() =>
              void run(async () => {
                await edit({
                  action: "delete",
                  sceneId: deleting.id,
                  confirmActive: true,
                });
                setDeleting(null);
              })
            }
          >
            {zh ? "确认移除" : "Remove scene"}
          </button>
          <button type="button" onClick={() => setDeleting(null)}>
            {zh ? "取消" : "Cancel"}
          </button>
        </div>
      )}
      {selected && (
        <section className="prefs-card desktop-pet-card pet-scene-detail">
          <header className="pet-library-toolbar">
            <h4>
              {zh ? "编辑场景" : "Edit scene"} · {selected.name}
            </h4>
            <button
              type="button"
              disabled={working}
              onClick={() => setEditing(null)}
            >
              {zh ? "收起" : "Close"}
            </button>
          </header>
          <form
            className="pet-scene-editor"
            onSubmit={(e) => {
              e.preventDefault();
              void run(() =>
                edit({ action: "rename", sceneId: selected.id, name }),
              );
            }}
          >
            <label>
              {zh ? "场景名称" : "Scene name"}
              <input
                value={name}
                maxLength={80}
                disabled={disabled}
                onChange={(e) => setName(e.currentTarget.value)}
              />
            </label>
            <button type="submit" disabled={disabled || !name.trim()}>
              {zh ? "保存名称" : "Save name"}
            </button>
          </form>
          <div className="pet-scene-actions">
            <button
              type="button"
              disabled={disabled}
              onClick={() => void run(() => bind(selected.id))}
            >
              {zh ? "从文件关联壁纸" : "Attach wallpaper file"}
            </button>
            <select
              aria-label={zh ? "最近使用的壁纸" : "Recent wallpapers"}
              value=""
              disabled={disabled}
              onFocus={loadRecent}
              onChange={(e) => {
                const sourcePath = e.currentTarget.value;
                if (sourcePath)
                  void run(() =>
                    mutate("bind_pet_scene_wallpaper", {
                      sceneId: selected.id,
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
          </div>
          <label className="desktop-pet-field">
            <span>{zh ? "AI 壁纸描述" : "AI wallpaper description"}</span>
            <textarea
              value={description}
              maxLength={2000}
              disabled={disabled}
              placeholder={
                zh ? "例如：温暖安静的森林小屋" : "A peaceful woodland home"
              }
              onChange={(e) => setDescription(e.currentTarget.value)}
            />
          </label>
          <label className="desktop-pet-preference-row">
            <span>
              {zh
                ? "壁纸中也画出宠物（默认只生成环境）"
                : "Include pet portrait (environment only by default)"}
            </span>
            <input
              type="checkbox"
              checked={includePet}
              disabled={disabled}
              onChange={(e) => setIncludePet(e.currentTarget.checked)}
            />
          </label>
          <div className="pet-scene-actions">
            <button
              type="button"
              disabled={disabled || !description.trim()}
              onClick={() => void run(() => generate(selected.id))}
            >
              {zh ? "生成 / 重试壁纸" : "Generate / retry wallpaper"}
            </button>
          </div>
          <p className="desktop-pet-model">
            {zh
              ? "调用一次图片模型；只用于 Astro 背景，不更改系统壁纸。"
              : "One image request. Astro background only, not system wallpaper."}
          </p>
          <label className="desktop-pet-preference-row">
            <span>
              {zh ? "本场景自定义配置" : "Override pet defaults for this scene"}
            </span>
            <input
              type="checkbox"
              disabled={disabled}
              checked={!!selected.preferences}
              onChange={(e) => {
                const preferences = e.currentTarget.checked
                  ? pet.defaults
                  : null;
                void run(() =>
                  edit({
                    action: "set_preferences",
                    sceneId: selected.id,
                    preferences,
                  }),
                );
              }}
            />
          </label>
          {selected.preferences ? (
            <PetPreferencesEditor
              value={selected.preferences}
              zh={zh}
              disabled={disabled}
              onSave={(preferences) =>
                run(() =>
                  edit({
                    action: "set_preferences",
                    sceneId: selected.id,
                    preferences,
                  }),
                )
              }
            />
          ) : (
            <p className="desktop-pet-model">
              {zh
                ? "继承宠物默认配置。修改宠物默认值后，本场景会同步使用新值。"
                : "Inherits the pet defaults, including future changes."}
            </p>
          )}
          <div className="pet-scene-actions">
            <button
              type="button"
              disabled={disabled || state.activePetId !== pet.id}
              onClick={() =>
                void run(() =>
                  edit({ action: "capture_preferences", sceneId: selected.id }),
                )
              }
            >
              {zh
                ? "将桌面当前大小与位置保存到此场景"
                : "Capture live size and placement"}
            </button>
          </div>
        </section>
      )}
      <div className="pet-scene-list">
        {scenesForPet(scenes, pet.id).map((scene) => (
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
                  reducedMotion
                  className="pet-scene-pet"
                  label={scene.name}
                />
              ) : (
                <img
                  className="pet-scene-pet"
                  src={resolveMediaSrc(scene.pet.petPath) || undefined}
                  alt={scene.name}
                />
              )}
            </div>
            <h4>
              {scene.name}{" "}
              {state.activeSceneId === scene.id && (
                <span className="pet-scene-active">
                  {zh ? "当前配置" : "Applied configuration"}
                </span>
              )}
            </h4>
            <p>
              {scene.wallpaperPath
                ? zh
                  ? "桌宠 + 配套壁纸"
                  : "Pet + wallpaper"
                : zh
                  ? "不更换壁纸"
                  : "Wallpaper unchanged"}{" "}
              ·{" "}
              {scene.preferences
                ? zh
                  ? "场景自定义"
                  : "Scene overrides"
                : zh
                  ? "继承默认"
                  : "Inherits defaults"}
            </p>
            <p>
              {petScalePercent((scene.preferences ?? pet.defaults).scale)}% ·{" "}
              {(scene.preferences ?? pet.defaults).behavior.quietMode
                ? zh
                  ? "安静模式"
                  : "Quiet"
                : zh
                  ? "自动活动"
                  : "Active"}
            </p>
            <div className="pet-scene-card-footer">
              <button
                type="button"
                className="desktop-pet-import-package"
                disabled={disabled}
                onClick={() =>
                  void run(() =>
                    mutate("apply_pet_scene", {
                      sceneId: scene.id,
                      mode: scene.wallpaperPath ? "all" : "pet",
                    }),
                  )
                }
              >
                {zh ? "应用整套" : "Apply scene"}
              </button>
              <button
                type="button"
                aria-label={zh ? "收藏场景" : "Favorite scene"}
                aria-pressed={scene.favorite}
                disabled={disabled}
                onClick={() =>
                  void run(() =>
                    edit({
                      action: "favorite",
                      sceneId: scene.id,
                      favorite: !scene.favorite,
                    }),
                  )
                }
              >
                <Star
                  size={16}
                  fill={scene.favorite ? "currentColor" : "none"}
                />
              </button>
              <PetMoreMenu label={zh ? "场景更多操作" : "More scene actions"}>
                <button
                  type="button"
                  disabled={disabled}
                  onClick={() => {
                    setEditing(scene.id);
                    setName(scene.name);
                  }}
                >
                  {zh ? "编辑场景" : "Edit scene"}
                </button>
                <button
                  type="button"
                  disabled={disabled}
                  onClick={() =>
                    void run(() =>
                      mutate("apply_pet_scene", {
                        sceneId: scene.id,
                        mode: "pet",
                      }),
                    )
                  }
                >
                  {zh ? "仅换宠物" : "Pet only"}
                </button>
                <button
                  type="button"
                  disabled={disabled || !scene.wallpaperPath}
                  onClick={() =>
                    void run(() =>
                      mutate("apply_pet_scene", {
                        sceneId: scene.id,
                        mode: "wallpaper",
                      }),
                    )
                  }
                >
                  {zh ? "仅换壁纸" : "Wallpaper only"}
                </button>
                <button
                  type="button"
                  disabled={disabled}
                  onClick={() =>
                    void run(() =>
                      edit({
                        action: "duplicate",
                        sceneId: scene.id,
                        name:
                          scene.name.slice(0, 65) + (zh ? " 副本" : " copy"),
                      }),
                    )
                  }
                >
                  {zh ? "复制场景" : "Duplicate scene"}
                </button>
                <button
                  type="button"
                  disabled={disabled}
                  onClick={() => void run(() => exportScene(scene.id))}
                >
                  {zh ? "导出场景包" : "Export scene"}
                </button>
                <button
                  type="button"
                  disabled={disabled}
                  onClick={() => setDeleting(scene)}
                >
                  {zh ? "移除场景…" : "Remove scene…"}
                </button>
              </PetMoreMenu>
            </div>
          </article>
        ))}
      </div>
      {loading && (
        <p className="pet-detail-loading" role="status">
          {zh ? "正在读取场景…" : "Loading scenes…"}
        </p>
      )}
      {!loading &&
        !error &&
        !loadError &&
        !creating &&
        !scenesForPet(scenes, pet.id).length && (
          <div className="pet-library-empty pet-detail-empty">
            <strong>{zh ? "还没有专属场景" : "No saved scenes yet"}</strong>
            <p>
              {zh
                ? "点击「新增场景」为它保存一个家。没有场景也可以直接使用宠物。"
                : "Add a scene to save a home. You can use the pet without creating a scene."}
            </p>
          </div>
        )}
    </section>
  );
}
