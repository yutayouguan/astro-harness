import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import {
  Download,
  Images,
  PawPrint,
  RefreshCw,
  Star,
  Upload,
} from "lucide-react";
import { useReducedMotion } from "framer-motion";
import DesktopPetCanvas from "../desktop-pet/DesktopPetCanvas";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import { groupPetScenes, type PetScene } from "../../lib/ui/petScene";
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
  const [notice, setNotice] = useState("");
  const [recent, setRecent] = useState<WallpaperAsset[]>([]);
  const [editor, setEditor] = useState<{
    action: "rename" | "rename_pet" | "duplicate";
    sceneId: string;
    name: string;
  } | null>(null);
  const [deleting, setDeleting] = useState<PetScene | null>(null);
  const [generation, setGeneration] = useState<string | null>(null);
  const workingRef = useRef(false);
  const reduced = useReducedMotion();
  const disabled = busy || working;
  const groups = groupPetScenes(scenes);
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
    if (busy || workingRef.current) return;
    workingRef.current = true;
    setWorking(true);
    setError("");
    setNotice("");
    try {
      await operation();
    } catch (e) {
      setError(String(e));
    } finally {
      workingRef.current = false;
      setWorking(false);
    }
  }
  function edit(request: Record<string, unknown>) {
    return mutate("edit_pet_scene", { request });
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
  async function exportScene(sceneId: string) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const destination = await open({
      directory: true,
      multiple: false,
      title: zh ? "选择导出文件夹" : "Choose export folder",
    });
    if (typeof destination === "string") {
      const path = await invoke<string>("export_pet_scene", {
        sceneId,
        destination,
      });
      setNotice(
        (zh
          ? "已导出（不含原始照片）："
          : "Exported without the source photo: ") + path,
      );
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
  async function generateWallpaper(sceneId: string) {
    const requestId = crypto.randomUUID();
    setGeneration(requestId);
    try {
      await mutate("generate_pet_scene_wallpaper", {
        sceneId,
        requestId,
        description:
          description.trim() ||
          (zh ? "温暖安静的森林小屋" : "A warm peaceful woodland home"),
        includePet,
      });
    } finally {
      setGeneration(null);
    }
  }

  return (
    <section className="prefs-card pet-scene-library" aria-busy={disabled}>
      <header className="desktop-pet-card-head">
        <Images size={20} aria-hidden />
        <div>
          <h3>{zh ? "我的宠物 · 它的场景" : "My pets · their scenes"}</h3>
          <p>
            {zh
              ? "给同一只宠物换一个家。新增场景复用形象，切换不消耗生成额度。"
              : "Give your pet another home. New scenes reuse its identity; switching is free."}
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
          onChange={(e) => {
            const enabled = e.currentTarget.checked;
            void run(() =>
              mutate("set_pet_scene_follow_wallpaper", { enabled }),
            );
          }}
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
              requestId: crypto.randomUUID(),
              petName: null,
            }),
          )
        }
      >
        <PawPrint size={16} />
        {zh ? "收藏当前桌宠" : "Save current pet"}
      </button>
      {working && (
        <p role="status">
          {generation
            ? zh
              ? "正在生成配套壁纸…"
              : "Generating wallpaper…"
            : zh
              ? "正在处理场景…"
              : "Updating scene…"}
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
                    ? "已请求取消，远端请求可能仍计费。"
                    : "Cancellation requested; submitted requests may still be billed.",
                ),
              )
              .catch((e) => setError(String(e)))
          }
        >
          {zh ? "取消生成" : "Cancel generation"}
        </button>
      )}
      {error && (
        <p className="desktop-pet-error" role="alert">
          {error}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
      {editor && (
        <form
          className="pet-scene-editor"
          onSubmit={(e) => {
            e.preventDefault();
            const request = editor;
            void run(async () => {
              await edit(request);
              setEditor(null);
            });
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") setEditor(null);
          }}
        >
          <label>
            {editor.action === "rename_pet"
              ? zh
                ? "宠物名字（所有场景同步）"
                : "Pet name (all scenes)"
              : editor.action === "duplicate"
                ? zh
                  ? "新场景名称（复用这只宠物）"
                  : "New scene (same pet)"
                : zh
                  ? "场景名称"
                  : "Scene name"}
            <input
              autoFocus
              value={editor.name}
              maxLength={80}
              disabled={disabled}
              onChange={(e) =>
                setEditor({ ...editor, name: e.currentTarget.value })
              }
            />
          </label>
          <button type="submit" disabled={disabled || !editor.name.trim()}>
            {zh ? "保存" : "Save"}
          </button>
          <button
            type="button"
            disabled={disabled}
            onClick={() => setEditor(null)}
          >
            {zh ? "取消" : "Cancel"}
          </button>
        </form>
      )}
      {deleting && (
        <div className="pet-scene-editor" role="alert">
          <p>
            {deleting.inUse
              ? zh
                ? "这个场景的桌宠或壁纸正在使用。移出收藏后，当前画面和素材文件仍会保留。"
                : "This pet or wallpaper is in use. Removing the scene keeps the current visuals and asset files."
              : zh
                ? "将这个场景移出收藏？素材文件会保留。"
                : "Remove this scene from the library? Asset files are kept."}{" "}
            — {deleting.name}
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
          <button
            type="button"
            disabled={disabled}
            onClick={() => setDeleting(null)}
          >
            {zh ? "取消" : "Cancel"}
          </button>
        </div>
      )}
      {groups.map((group) => (
        <section className="pet-identity-group" key={group.petPath}>
          <header className="pet-identity-header">
            <h4>
              {group.name || (zh ? "未命名宠物" : "Unnamed pet")}{" "}
              <small>
                {group.scenes.length} {zh ? "个场景" : "scenes"}
              </small>
            </h4>
            <button
              type="button"
              disabled={disabled}
              onClick={() =>
                setEditor({
                  action: "rename_pet",
                  sceneId: group.scenes[0].id,
                  name: group.name || "",
                })
              }
            >
              {zh ? "给宠物改名" : "Rename pet"}
            </button>
            <button
              type="button"
              disabled={disabled}
              onClick={() =>
                setEditor({
                  action: "duplicate",
                  sceneId: group.scenes[0].id,
                  name: "",
                })
              }
            >
              {zh ? "＋ 新增它的场景" : "+ Add a home"}
            </button>
          </header>
          <div className="pet-scene-list">
            {group.scenes.map((scene) => (
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
                <h4>
                  {scene.name}{" "}
                  {scene.inUse && (
                    <span className="pet-scene-active">
                      {zh ? "使用中" : "In use"}
                    </span>
                  )}
                </h4>
                <p>
                  {scene.wallpaperPath
                    ? zh
                      ? "桌宠 + 配套壁纸"
                      : "Pet + wallpaper"
                    : zh
                      ? "桌宠已保存，可补生成或关联壁纸"
                      : "Pet saved. Generate or attach a wallpaper."}
                </p>
                {scene.pet.provider && (
                  <p>
                    {scene.pet.provider} · {scene.pet.model}
                  </p>
                )}
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
                          mutate("apply_pet_scene", {
                            sceneId: scene.id,
                            mode,
                          }),
                        )
                      }
                    >
                      {zh
                        ? {
                            all: "应用整套",
                            pet: "仅桌宠",
                            wallpaper: "仅壁纸",
                          }[mode]
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
                    {zh ? "关联壁纸" : "Attach wallpaper"}
                  </button>
                  <button
                    type="button"
                    disabled={disabled}
                    onClick={() => void run(() => generateWallpaper(scene.id))}
                  >
                    <RefreshCw size={14} />
                    {zh ? "生成 / 重试壁纸" : "Generate / retry wallpaper"}
                  </button>
                </div>
                <div className="pet-scene-actions">
                  <button
                    type="button"
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
                      size={14}
                      fill={scene.favorite ? "currentColor" : "none"}
                    />
                    {zh ? "收藏" : "Favorite"}
                  </button>
                  <button
                    type="button"
                    disabled={disabled}
                    onClick={() =>
                      setEditor({
                        action: "rename",
                        sceneId: scene.id,
                        name: scene.name,
                      })
                    }
                  >
                    {zh ? "改名" : "Rename"}
                  </button>
                  <button
                    type="button"
                    disabled={disabled}
                    onClick={() => void run(() => exportScene(scene.id))}
                  >
                    <Download size={14} />
                    {zh ? "导出" : "Export"}
                  </button>
                  <button
                    type="button"
                    disabled={disabled || state.petPath !== scene.pet.petPath}
                    title={
                      zh
                        ? "保存当前大小、位置和安静偏好到此场景"
                        : "Save current size, placement and behavior to this scene"
                    }
                    onClick={() =>
                      void run(() =>
                        edit({
                          action: "capture_preferences",
                          sceneId: scene.id,
                        }),
                      )
                    }
                  >
                    {zh ? "保存当前偏好" : "Save current preferences"}
                  </button>
                  <button
                    type="button"
                    disabled={disabled}
                    onClick={() => setDeleting(scene)}
                  >
                    {zh ? "移除" : "Remove"}
                  </button>
                </div>
              </article>
            ))}
          </div>
        </section>
      ))}
      {!scenes.length && (
        <p>
          {zh
            ? "生成新桌宠或收藏当前桌宠后，它和它的场景会出现在这里。"
            : "Generate or save a pet to start its scene collection."}
        </p>
      )}
    </section>
  );
}
