import { useEffect, useRef, useState } from "react";
import { ArrowLeft, Check, PawPrint, Plus, Search } from "lucide-react";
import { useReducedMotion } from "framer-motion";
import {
  availablePetActions,
  petActionLabel,
} from "../../lib/ui/petActionCatalog";
import type { DesktopPetState } from "../../lib/ui/desktopPetState";
import { filterPets, type PetRecord } from "../../lib/ui/petLibrary";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import DesktopPetCanvas from "../desktop-pet/DesktopPetCanvas";
import { SegmentedTabs } from "../ui/SegmentedTabs";
import { SelectMenu } from "../ui/SelectMenu";
import PetPreferencesEditor from "./PetPreferencesEditor";
import { supportsPetRoaming } from "../../lib/ui/petRoaming";
import PetMoreMenu from "./PetMoreMenu";
import PetSceneLibrary from "./PetSceneLibrary";
import "../../styles/features/pet-detail.css";

export function PetPortrait({
  pet,
  active,
  action = "idle",
  className = "pet-library-portrait",
}: {
  pet: PetRecord;
  active: boolean;
  action?: string;
  className?: string;
}) {
  const reduced = useReducedMotion();
  const identity = pet.identity,
    src = resolveMediaSrc(identity.petPath) || "";
  const selectedAction =
    active && availablePetActions(identity).includes(action) ? action : "idle";
  return ([2, 3].includes(identity.spriteVersionNumber ?? 0)) ? (
    <DesktopPetCanvas
      src={src}
      state={selectedAction === "kneading" ? "running" : "idle"}
      blinkProfile={
        identity.petPath.includes("builtin-naitang-") ? "naitang" : undefined
      }
      motionClips={active ? identity.motionClips : undefined}
      motionName={
        identity.motionClips?.[selectedAction] ? selectedAction : undefined
      }
      groomingSrc={
        active ? resolveMediaSrc(identity.groomingPath) || undefined : undefined
      }
      clip={
        selectedAction === "grooming" &&
        !identity.motionClips?.grooming &&
        identity.groomingPath
          ? "grooming"
          : undefined
      }
      repeatMotion
      reducedMotion={!!reduced || !active}
      className={className}
      label={identity.displayName || "Pet"}
    />
  ) : (
    <img src={src} alt={identity.displayName || "Pet"} className={className} />
  );
}

export default function PetLibraryPanel({
  state,
  mutate,
  busy,
  zh,
  active,
  onCreate,
}: {
  state: DesktopPetState;
  mutate: (
    command: string,
    args: Record<string, unknown>,
  ) => Promise<DesktopPetState>;
  busy: boolean;
  zh: boolean;
  active: boolean;
  onCreate: () => void;
}) {
  const [selected, setSelected] = useState<string | null>(null);
  const [detailTab, setDetailTab] = useState("scenes");
  const [query, setQuery] = useState("");
  const [source, setSource] = useState("all");
  const [action, setAction] = useState("idle");
  const [error, setError] = useState("");
  const [working, setWorking] = useState(false);
  const lock = useRef(false);
  const sceneCreateButton = useRef<HTMLButtonElement>(null);
  const [editor, setEditor] = useState<{
    action: "rename" | "add_scene";
    name: string;
  } | null>(null);
  const [deleting, setDeleting] = useState<number | null>(null);
  const pets = state.pets ?? [],
    pet = pets.find((p) => p.id === selected);
  const count = (id: string) =>
    state.scenes.filter((s) => s.pet?.petId === id).length;
  const disabled = busy || working;
  function cancelEditor() {
    const restoreSceneFocus = editor?.action === "add_scene";
    setEditor(null);
    if (restoreSceneFocus)
      requestAnimationFrame(() => sceneCreateButton.current?.focus());
  }
  useEffect(() => {
    setAction("idle");
    setDetailTab("scenes");
    setEditor(null);
    setDeleting(null);
  }, [selected]);
  async function run(command: string, args: Record<string, unknown>) {
    if (disabled || lock.current) return;
    lock.current = true;
    setWorking(true);
    setError("");
    try {
      await mutate(command, args);
      const requestAction = (args.request as { action?: string } | undefined)
        ?.action;
      if (
        command === "edit_pet_library" &&
        (requestAction === "rename" || requestAction === "add_scene")
      ) {
        setEditor(null);
        if (requestAction === "add_scene") {
          requestAnimationFrame(() => sceneCreateButton.current?.focus());
        }
      }
      if (command === "edit_pet_library" && requestAction === "delete") {
        setDeleting(null);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      lock.current = false;
      setWorking(false);
    }
  }
  const editorForm = editor && pet && (
    <form
      className="pet-scene-editor pet-detail-name-editor"
      onKeyDown={(e) => {
        if (e.key === "Escape" && !e.nativeEvent.isComposing && !disabled) {
          e.preventDefault();
          cancelEditor();
        }
      }}
      onSubmit={(e) => {
        e.preventDefault();
        void run("edit_pet_library", {
          request: { ...editor, petId: pet.id },
        });
      }}
    >
      <label>
        {editor.action === "rename"
          ? zh
            ? "宠物名字"
            : "Pet name"
          : zh
            ? "新场景名称"
            : "Scene name"}
        <input
          autoFocus
          maxLength={80}
          disabled={disabled}
          placeholder={
            editor.action === "add_scene"
              ? zh
                ? "例如：窗边午睡"
                : "For example: Window nook"
              : undefined
          }
          value={editor.name}
          onChange={(e) =>
            setEditor({ ...editor, name: e.currentTarget.value })
          }
        />
      </label>
      <button type="submit" disabled={disabled || !editor.name.trim()}>
        {zh ? "保存" : "Save"}
      </button>
      <button type="button" disabled={disabled} onClick={cancelEditor}>
        {zh ? "取消" : "Cancel"}
      </button>
    </form>
  );
  return (
    <div
      className={`pet-library${pet ? " pet-detail-view" : ""}`}
      aria-busy={working}
    >
      {error && (
        <p role="alert" className="desktop-pet-error">
          {error}
        </p>
      )}
      {!pet ? (
        <>
          <header className="pet-library-toolbar">
            <div className="pet-library-heading">
              <h3>{zh ? "我的伙伴" : "My companions"}</h3>
              <p>
                {zh
                  ? "管理伙伴的场景与动作，浏览不会切换桌面。"
                  : "Manage scenes and motion without switching your desktop."}
              </p>
            </div>
            <label className="pet-library-search">
              <Search size={16} aria-hidden />
              <input
                type="search"
                value={query}
                aria-label={zh ? "搜索宠物" : "Search pets"}
                placeholder={zh ? "搜索宠物名字…" : "Search companions…"}
                onChange={(e) => setQuery(e.currentTarget.value)}
              />
            </label>
            <SelectMenu
              className="pet-library-filter"
              value={source}
              aria-label={zh ? "宠物来源" : "Pet source"}
              onChange={setSource}
              options={[
                { value: "all", label: zh ? "全部宠物" : "All pets" },
                { value: "builtin", label: zh ? "内置宠物" : "Built-in" },
                { value: "custom", label: zh ? "自定义宠物" : "Custom" },
              ]}
            />
            <button
              type="button"
              className="desktop-pet-import-package"
              onClick={onCreate}
            >
              <Plus size={16} />
              {zh ? "添加宠物" : "Add pet"}
            </button>
          </header>
          <div className="pet-library-grid">
            {filterPets(pets, query, source).map((item) => {
              const current = state.activePetId === item.id;
              const actions = availablePetActions(item.identity);
              const sceneCount = count(item.id);
              return (
                <article
                  key={item.id}
                  className="pet-library-card"
                  data-current={current}
                  aria-label={
                    item.identity.displayName ||
                    (zh ? "未命名宠物" : "Unnamed pet")
                  }
                >
                  <button
                    type="button"
                    className="pet-library-open"
                    onClick={() => setSelected(item.id)}
                    aria-label={`${zh ? "管理" : "Manage"} ${item.identity.displayName}`}
                  >
                    <PetPortrait pet={item} active={false} />
                    <span className="pet-library-card-copy">
                      <strong>
                        {item.identity.displayName ||
                          (zh ? "未命名宠物" : "Unnamed pet")}
                      </strong>
                      <span className="pet-library-meta">
                        {item.builtin
                          ? zh
                            ? "内置"
                            : "Built-in"
                          : zh
                            ? "自定义"
                            : "Custom"}{" "}
                        ·{" "}
                        {([2, 3].includes(item.identity.spriteVersionNumber ?? 0))
                          ? zh
                            ? "动画"
                            : "Animated"
                          : zh
                            ? "静态"
                            : "Static"}
                        <span>
                          {sceneCount
                            ? `${sceneCount} ${zh ? "个场景" : "scenes"}`
                            : zh
                              ? "尚未创建场景"
                              : "No scenes yet"}
                        </span>
                      </span>
                      <span className="pet-library-action-tags">
                        {(actions.length
                          ? actions.slice(0, 3)
                          : ([2, 3].includes(item.identity.spriteVersionNumber ?? 0))
                            ? ["idle"]
                            : []
                        ).map((name) => (
                          <span
                            key={name}
                            title={petActionLabel(name, zh ? "zh" : "en")}
                          >
                            {petActionLabel(name, zh ? "zh" : "en")}
                          </span>
                        ))}
                        {actions.length > 3 && (
                          <span
                            title={actions
                              .slice(3)
                              .map((name) =>
                                petActionLabel(name, zh ? "zh" : "en"),
                              )
                              .join(" · ")}
                          >
                            +{actions.length - 3}
                          </span>
                        )}
                      </span>
                    </span>
                  </button>
                  <div className="pet-library-card-footer">
                    <button
                      type="button"
                      className="pet-library-manage"
                      aria-label={
                        zh
                          ? `管理${item.identity.displayName}的场景`
                          : `Manage ${item.identity.displayName} scenes`
                      }
                      onClick={() => setSelected(item.id)}
                    >
                      {zh ? "管理场景" : "Manage scenes"}
                    </button>
                    {current ? (
                      <span className="pet-library-current-badge">
                        <Check size={14} aria-hidden />
                        {zh ? "当前桌宠" : "Current pet"}
                      </span>
                    ) : (
                      <button
                        type="button"
                        className="pet-library-switch"
                        disabled={disabled}
                        title={
                          zh
                            ? "应用这只宠物的默认大小与位置，不更换壁纸。"
                            : "Apply this pet's default size and placement. Keep the wallpaper."
                        }
                        onClick={() =>
                          void run("apply_library_pet", { petId: item.id })
                        }
                      >
                        {zh ? "切换到桌面" : "Use on desktop"}
                      </button>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
          {!filterPets(pets, query, source).length && (
            <p className="pet-library-empty">
              {zh
                ? "没有找到宠物。试试其他名字，或创建你的第一只伙伴。"
                : "No pets found. Try another name or create a companion."}
            </p>
          )}
        </>
      ) : (
        <>
          <button
            className="pet-library-back"
            type="button"
            onClick={() => setSelected(null)}
          >
            <ArrowLeft size={16} />
            {zh ? "宠物库" : "Library"} / {pet.identity.displayName}
          </button>
          <header className="pet-detail-header">
            <PetPortrait
              pet={pet}
              active={false}
              action={action}
              className="pet-detail-portrait"
            />
            <div>
              <h3>{pet.identity.displayName}</h3>
              <p title={pet.identity.description || undefined}>
                {pet.builtin
                  ? zh
                    ? "内置伙伴"
                    : "Built-in companion"
                  : zh
                    ? "自定义伙伴"
                    : "Custom companion"}
                {" · "}
                {([2, 3].includes(pet.identity.spriteVersionNumber ?? 0))
                  ? zh
                    ? "动画"
                    : "Animated"
                  : zh
                    ? "静态"
                    : "Static"}
                {" · "}
                {count(pet.id)
                  ? `${count(pet.id)} ${zh ? "个场景" : "scenes"}`
                  : zh
                    ? "尚未创建场景"
                    : "No scenes yet"}
              </p>
              <small>
                {state.activePetId === pet.id
                  ? zh
                    ? "当前桌宠"
                    : "Current pet"
                  : zh
                    ? "正在浏览"
                    : "Previewing"}
                {" · "}
                {zh
                  ? "浏览不会改变桌面"
                  : "Browsing does not change your desktop"}
              </small>
            </div>
            <div className="pet-detail-actions">
              <button
                type="button"
                className="desktop-pet-import-package"
                disabled={disabled}
                onClick={() => void run("apply_library_pet", { petId: pet.id })}
                aria-label={zh ? "应用宠物默认配置" : "Apply pet defaults"}
                title={
                  zh
                    ? "应用已保存的大小、位置和行为，不更换壁纸"
                    : "Apply saved size, placement and behavior without changing wallpaper"
                }
              >
                <PawPrint size={16} />
                {zh ? "应用默认配置" : "Apply defaults"}
              </button>
              <PetMoreMenu label={zh ? "宠物更多操作" : "More pet actions"}>
                <button
                  type="button"
                  disabled={disabled}
                  onClick={() =>
                    setEditor({
                      action: "rename",
                      name: pet.identity.displayName || "",
                    })
                  }
                >
                  {zh ? "重命名" : "Rename"}
                </button>
                {!pet.builtin && (
                  <button
                    type="button"
                    disabled={disabled}
                    onClick={() => setDeleting(count(pet.id))}
                  >
                    {zh ? "移除宠物…" : "Remove pet…"}
                  </button>
                )}
              </PetMoreMenu>
            </div>
          </header>
          {editor?.action === "rename" && editorForm}
          {deleting !== null && (
            <div className="pet-scene-editor" role="alert">
              <p>
                {zh
                  ? `将移除「${pet.identity.displayName}」及 ${deleting} 个场景。若正在桌面显示会先隐藏；素材文件保留。`
                  : `Remove this pet and ${deleting} scenes? The active pet will be hidden; files are retained.`}
              </p>
              <button
                type="button"
                disabled={disabled}
                onClick={() =>
                  void run("edit_pet_library", {
                    request: {
                      action: "delete",
                      petId: pet.id,
                      confirmSceneCount: deleting,
                      confirmActive: true,
                    },
                  })
                }
              >
                {zh ? "确认移除" : "Confirm removal"}
              </button>
              <button type="button" onClick={() => setDeleting(null)}>
                {zh ? "取消" : "Cancel"}
              </button>
            </div>
          )}
          <SegmentedTabs
            className="pet-detail-tabs"
            aria-label={zh ? "宠物详情" : "Pet details"}
            value={detailTab}
            onValueChange={setDetailTab}
            size="sm"
            items={[
              {
                value: "scenes",
                label: zh ? "场景" : "Scenes",
                panelId: "pet-detail-scenes",
              },
              {
                value: "actions",
                label: zh ? "动作与偏好" : "Motion & defaults",
                panelId: "pet-detail-actions",
              },
            ]}
          />
          <div
            hidden={detailTab !== "scenes"}
            role="tabpanel"
            aria-label={zh ? "场景" : "Scenes"}
            id="pet-detail-scenes"
          >
            <div className="pet-library-toolbar pet-detail-scene-toolbar">
              <div>
                <h4>{zh ? "专属场景" : "Saved scenes"}</h4>
                <p>
                  {zh
                    ? "保存壁纸、大小与位置；新场景继承宠物默认配置。"
                    : "Save wallpaper, size and placement. New scenes inherit pet defaults."}
                </p>
              </div>
              <button
                type="button"
                disabled={disabled || editor?.action === "add_scene"}
                ref={sceneCreateButton}
                onClick={() => setEditor({ action: "add_scene", name: "" })}
                aria-expanded={editor?.action === "add_scene"}
              >
                <Plus size={16} />
                {zh ? "新增场景" : "Add scene"}
              </button>
            </div>
            {editor?.action === "add_scene" && editorForm}
            <PetSceneLibrary
              creating={editor?.action === "add_scene"}
              key={pet.id}
              state={state}
              mutate={mutate}
              busy={disabled}
              zh={zh}
              pet={pet}
              active={active && detailTab === "scenes"}
            />
          </div>
          <div
            hidden={detailTab !== "actions"}
            role="tabpanel"
            aria-label={zh ? "动作与偏好" : "Motion & defaults"}
            id="pet-detail-actions"
            className="prefs-card desktop-pet-card pet-motion-settings"
          >
            <div className="pet-motion-preview">
              <header>
                <h4>{zh ? "动作预览" : "Motion preview"}</h4>
                <p>
                  {zh
                    ? "仅在此处播放，不影响桌面"
                    : "Preview here without changing your desktop"}
                </p>
              </header>
              <PetPortrait
                pet={pet}
                active={active && detailTab === "actions"}
                action={action}
                className="pet-motion-portrait"
              />
              <div className="pet-scene-actions">
                {(([2, 3].includes(pet.identity.spriteVersionNumber ?? 0))
                  ? ["idle", ...availablePetActions(pet.identity)]
                  : []
                ).map((name) => (
                  <button
                    key={name}
                    type="button"
                    aria-pressed={action === name}
                    onClick={() => setAction(name)}
                  >
                    {petActionLabel(name, zh ? "zh" : "en")}
                  </button>
                ))}
              </div>
            </div>
            <div className="pet-motion-preferences">
              <header className="desktop-pet-card-head">
                <div>
                  <h3>{zh ? "默认陪伴方式" : "Companion defaults"}</h3>
                  <p>
                    {zh
                      ? "大小、位置与日常习惯"
                      : "Size, placement and daily habits"}
                  </p>
                </div>
              </header>
              <p className="desktop-pet-model">
                {zh
                  ? state.activePetId === pet.id
                    ? "保存更新宠物默认配置；未单独配置的场景会继承它。"
                    : "保存只更新宠物默认配置，不立即改变桌面；未单独配置的场景会继承它。"
                  : state.activePetId === pet.id
                    ? "Saving updates pet defaults inherited by scenes without overrides."
                    : "Saving changes defaults only, not the live desktop. Scenes without overrides inherit these values."}
              </p>
              <PetPreferencesEditor
                value={pet.defaults}
                roamingSupported={supportsPetRoaming(pet.identity.motionClips)}
                onPlaceOnGround={state.activePetId === pet.id && state.enabled
                  ? () => mutate("place_desktop_pet_on_ground", { petId: pet.id }) : undefined}
                zh={zh}
                disabled={disabled}
                liveScale={
                  state.activePetId === pet.id ? state.scale : undefined
                }
                onApplyScale={
                  state.activePetId === pet.id
                    ? (scale) =>
                        mutate("set_desktop_pet_scale", {
                          scale,
                          petId: pet.id,
                        })
                    : undefined
                }
                onSave={(defaults) =>
                  run("edit_pet_library", {
                    request: {
                      action: "set_defaults",
                      petId: pet.id,
                      defaults,
                    },
                  })
                }
              />
            </div>
          </div>
        </>
      )}
    </div>
  );
}
