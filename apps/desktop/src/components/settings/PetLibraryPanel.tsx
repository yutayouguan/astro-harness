import { useEffect, useRef, useState } from "react";
import { ArrowLeft, PawPrint, Plus, Search } from "lucide-react";
import { useReducedMotion } from "framer-motion";
import type { DesktopPetState } from "../../lib/ui/desktopPetState";
import { filterPets, type PetRecord } from "../../lib/ui/petLibrary";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import DesktopPetCanvas from "../desktop-pet/DesktopPetCanvas";
import { SegmentedTabs } from "../ui/SegmentedTabs";
import PetPreferencesEditor from "./PetPreferencesEditor";
import PetMoreMenu from "./PetMoreMenu";
import PetSceneLibrary from "./PetSceneLibrary";

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
  return identity.spriteVersionNumber === 2 ? (
    <DesktopPetCanvas
      src={src}
      state={action === "kneading" ? "running" : "idle"}
      motionClips={identity.motionClips}
      motionName={identity.motionClips?.[action] ? action : undefined}
      groomingSrc={resolveMediaSrc(identity.groomingPath) || undefined}
      clip={
        action === "grooming" &&
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
      setEditor(null);
      setDeleting(null);
    } catch (e) {
      setError(String(e));
    } finally {
      lock.current = false;
      setWorking(false);
    }
  }
  return (
    <div className="pet-library" aria-busy={working}>
      {error && (
        <p role="alert" className="desktop-pet-error">
          {error}
        </p>
      )}
      {!pet ? (
        <>
          <header className="pet-library-toolbar">
            <div className="pet-library-heading">
              <h3>
                {zh ? "我的伙伴" : "My companions"}
                <span>{pets.length}</span>
              </h3>
              <p>
                {zh ? "一只宠物，多个专属场景" : "One companion, many homes"}
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
            <select
              value={source}
              aria-label={zh ? "宠物来源" : "Pet source"}
              onChange={(e) => setSource(e.currentTarget.value)}
            >
              <option value="all">{zh ? "全部宠物" : "All pets"}</option>
              <option value="builtin">{zh ? "内置" : "Built-in"}</option>
              <option value="custom">{zh ? "我的宠物" : "Custom"}</option>
            </select>
            <button
              type="button"
              className="desktop-pet-import-package"
              onClick={onCreate}
            >
              <Plus size={16} />
              {zh ? "创建 / 导入" : "Create / import"}
            </button>
          </header>
          <p className="desktop-pet-model">
            {zh
              ? "先选择一只宠物，管理它的场景。浏览与预览不会改变桌面。"
              : "Choose a pet to manage its scenes. Browsing never changes your desktop."}
          </p>
          <div className="pet-library-grid">
            {filterPets(pets, query, source).map((item) => (
              <article
                key={item.id}
                className="pet-library-card"
                data-current={state.activePetId === item.id}
              >
                <button
                  type="button"
                  className="pet-library-open"
                  onClick={() => setSelected(item.id)}
                  aria-label={`${zh ? "管理" : "Manage"} ${item.identity.displayName}`}
                >
                  <PetPortrait pet={item} active={false} />
                  <div className="pet-library-card-copy">
                    <strong>
                      {item.identity.displayName ||
                        (zh ? "未命名宠物" : "Unnamed pet")}
                    </strong>
                    <span>
                      {item.builtin
                        ? zh
                          ? "内置"
                          : "Built-in"
                        : zh
                          ? "自定义"
                          : "Custom"}{" "}
                      ·{" "}
                      {item.identity.spriteVersionNumber === 2
                        ? zh
                          ? "动画"
                          : "Animated"
                        : zh
                          ? "静态"
                          : "Static"}{" "}
                      · {count(item.id)} {zh ? "个场景" : "scenes"}
                    </span>
                    <p>
                      {item.identity.description ||
                        (zh
                          ? "为它创建场景，留下你的专属陪伴。"
                          : "Create a home for your companion.")}
                    </p>
                  </div>
                </button>
                <div className="pet-library-card-footer">
                  <small>
                    {state.activePetId === item.id
                      ? zh
                        ? "当前桌宠"
                        : "Current pet"
                      : ""}
                  </small>
                  <button
                    type="button"
                    disabled={disabled}
                    onClick={() =>
                      void run("apply_library_pet", { petId: item.id })
                    }
                  >
                    {zh ? "应用到桌面" : "Apply pet"}
                  </button>
                </div>
              </article>
            ))}
            {!query.trim() && source === "all" && (
              <button
                type="button"
                className="pet-library-add"
                onClick={onCreate}
              >
                <span className="desktop-pet-icon">
                  <Plus size={22} />
                </span>
                <strong>
                  {zh ? "迎接一位新伙伴" : "Meet your next companion"}
                </strong>
                <span>
                  {zh
                    ? "从宠物照片生成，或导入动画宠物包"
                    : "Create from a photo or import an animated pet"}
                </span>
              </button>
            )}
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
              <p>{pet.identity.description}</p>
              <small>
                {count(pet.id)}{" "}
                {zh
                  ? "个场景 · 当前仅为管理预览"
                  : "scenes · management preview"}
              </small>
            </div>
            <button
              type="button"
              className="desktop-pet-import-package"
              disabled={disabled}
              onClick={() => void run("apply_library_pet", { petId: pet.id })}
            >
              <PawPrint size={16} />
              {zh ? "应用宠物默认配置" : "Apply pet defaults"}
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
          </header>
          {editor && (
            <form
              className="pet-scene-editor"
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
                  value={editor.name}
                  onChange={(e) =>
                    setEditor({ ...editor, name: e.currentTarget.value })
                  }
                />
              </label>
              <button type="submit" disabled={disabled || !editor.name.trim()}>
                {zh ? "保存" : "Save"}
              </button>
              <button type="button" onClick={() => setEditor(null)}>
                {zh ? "取消" : "Cancel"}
              </button>
            </form>
          )}
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
            <div className="pet-library-toolbar">
              <p>
                {zh
                  ? "同一只宠物，不同的家。新场景继承宠物默认配置。"
                  : "Different homes, one companion. New scenes inherit pet defaults."}
              </p>
              <button
                type="button"
                disabled={disabled}
                onClick={() => setEditor({ action: "add_scene", name: "" })}
              >
                <Plus size={16} />
                {zh ? "新增场景" : "Add scene"}
              </button>
            </div>
            <PetSceneLibrary
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
              <PetPortrait
                pet={pet}
                active={active && detailTab === "actions"}
                action={action}
                className="pet-motion-portrait"
              />
              <div className="pet-scene-actions">
                {(pet.identity.spriteVersionNumber === 2
                  ? [
                      "idle",
                      ...Object.keys(pet.identity.motionClips ?? {}),
                      ...(pet.identity.groomingPath &&
                      !pet.identity.motionClips?.grooming
                        ? ["grooming"]
                        : []),
                    ]
                  : []
                ).map((name) => (
                  <button
                    key={name}
                    type="button"
                    aria-pressed={action === name}
                    onClick={() => setAction(name)}
                  >
                    {(
                      {
                        idle: zh ? "待机 / 眨眼" : "Idle / blink",
                        kneading: zh ? "踩奶" : "Knead",
                        grooming: zh ? "舔脚脚" : "Groom",
                      } as Record<string, string>
                    )[name] || name}
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
                  ? "保存只更新宠物默认配置，不立即改变桌面；未单独配置的场景会继承它。"
                  : "Saving changes defaults only, not the live desktop. Scenes without overrides inherit these values."}
              </p>
              <PetPreferencesEditor
                value={pet.defaults}
                zh={zh}
                disabled={disabled}
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
