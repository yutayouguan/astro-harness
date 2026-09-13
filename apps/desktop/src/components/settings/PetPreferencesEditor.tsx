import { useEffect, useId, useState } from "react";
import {
  DESKTOP_PET_SCALE,
  petScalePercent,
} from "../../lib/ui/desktopPetState";
import type { PetDefaults } from "../../lib/ui/petLibrary";

export default function PetPreferencesEditor({
  value,
  zh,
  disabled,
  onSave,
  liveScale,
  onApplyScale,
  roamingSupported = false,
  onPlaceOnGround,
}: {
  value: PetDefaults;
  zh: boolean;
  disabled: boolean;
  onSave: (value: PetDefaults) => Promise<unknown>;
  liveScale?: number;
  onApplyScale?: (scale: number) => Promise<unknown>;
  roamingSupported?: boolean;
  onPlaceOnGround?: () => Promise<unknown>;
}) {
  const [draft, setDraft] = useState(value);
  const fieldId = useId();
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState("");
  const [scaling, setScaling] = useState(false);
  const [scaleStatus, setScaleStatus] = useState("");
  const signature = JSON.stringify(value);
  const dirty = JSON.stringify(draft) !== signature;
  const unavailable = disabled || saving || scaling;
  useEffect(() => {
    setDraft(JSON.parse(signature) as PetDefaults);
    setSaveError("");
  }, [signature]);
  const change = (next: PetDefaults) => {
    setDraft(next);
    setSaveError("");
  };
  async function save() {
    if (unavailable || !dirty) return;
    setSaving(true);
    try {
      await onSave(draft);
    } catch (error) {
      setSaveError(String(error));
    } finally {
      setSaving(false);
    }
  }
  async function applyScale(scale: number) {
    if (!onApplyScale || unavailable || scale === liveScale) return;
    setScaling(true);
    setScaleStatus("");
    setSaveError("");
    try {
      await onApplyScale(scale);
      setScaleStatus(
        zh
          ? "桌面大小已更新；保存配置可设为默认。"
          : "Desktop size updated. Save preferences to make it the default.",
      );
    } catch (error) {
      setSaveError(String(error));
    } finally {
      setScaling(false);
    }
  }
  return (
    <form
      className="pet-defaults-editor"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <label className="desktop-pet-size-row">
        <span>{zh ? "桌宠大小" : "Pet size"}</span>
        <input
          type="range"
          min={DESKTOP_PET_SCALE.min}
          max={DESKTOP_PET_SCALE.max}
          step={DESKTOP_PET_SCALE.step}
          value={draft.scale}
          disabled={unavailable}
          aria-valuetext={`${petScalePercent(draft.scale)}%`}
          aria-describedby={`${fieldId}-scale-hint`}
          onPointerDown={(e) => e.currentTarget.setPointerCapture(e.pointerId)}
          onPointerUp={(e) => void applyScale(Number(e.currentTarget.value))}
          onKeyUp={(e) => {
            if (
              [
                "ArrowLeft",
                "ArrowRight",
                "ArrowUp",
                "ArrowDown",
                "Home",
                "End",
                "PageUp",
                "PageDown",
              ].includes(e.key)
            )
              void applyScale(Number(e.currentTarget.value));
          }}
          onChange={(e) =>
            change({ ...draft, scale: Number(e.currentTarget.value) })
          }
        />
        <output>{petScalePercent(draft.scale)}%</output>
      </label>
      <p className="pet-size-hint" id={`${fieldId}-scale-hint`}>
        {onApplyScale
          ? zh
            ? "松手或方向键调整当前桌宠；保存可设为默认大小。"
            : "Release or use arrow keys to resize the current pet; save to set the default."
          : zh
            ? "调整保存的大小；保存后在下次应用时使用。"
            : "Edit the saved size; save and apply to use it."}
      </p>
      <span className="sr-only" role="status">
        {scaling
          ? zh
            ? "正在调整桌面大小…"
            : "Updating desktop size…"
          : scaleStatus}
      </span>
      {(
        [
          [
            "positionLocked",
            zh ? "锁定位置" : "Lock position",
            zh
              ? "防止误拖，仍可从菜单解锁。"
              : "Prevent accidental dragging; unlock from the menu.",
          ],
          [
            "snapToEdge",
            zh ? "贴边吸附" : "Snap to edges",
            zh
              ? "拖到屏幕边缘附近时自动贴边。"
              : "Snap to the work-area edge after dragging nearby.",
          ],
          [
            "quietMode",
            zh ? "安静模式" : "Quiet mode",
            zh
              ? "保留待机与眨眼，暂停自动大动作。"
              : "Keep idle and blinking; pause automatic activity.",
          ],
          [
            "roamingEnabled",
            zh ? "底部自主漫游" : "Roam along the bottom",
            roamingSupported
              ? zh
                ? "默认关闭；仅沿当前屏幕底边行走，锁定、安静或交互时暂停。"
                : "Off by default. Walk along this screen's bottom; pause while locked, quiet or interacting."
              : zh
                ? "此宠物尚未提供可用的 APNG 步态数据。"
                : "This pet does not provide APNG walking metadata yet.",
          ],
        ] as const
      ).map(([key, label, hint]) => (
        <div className="pet-detail-preference-row" key={key}>
          <label htmlFor={`${fieldId}-${key}`}>
            <strong>{label}</strong>
            <small id={`${fieldId}-${key}-hint`}>{hint}</small>
          </label>
          <button
            type="button"
            role="switch"
            className="prefs-switch"
            id={`${fieldId}-${key}`}
            aria-label={label}
            aria-describedby={`${fieldId}-${key}-hint`}
            aria-checked={draft.behavior[key]}
            disabled={
              unavailable || (key === "roamingEnabled" && !roamingSupported)
            }
            onClick={() =>
              change({
                ...draft,
                behavior: { ...draft.behavior, [key]: !draft.behavior[key] },
              })
            }
          >
            <span className="prefs-switch-thumb" />
          </button>
        </div>
      ))}
      {onPlaceOnGround && (
        <button
          type="button"
          className="pet-detail-clear-position"
          disabled={
            unavailable || !roamingSupported || draft.behavior.positionLocked
          }
          onClick={() => {
            setSaving(true);
            setSaveError("");
            void onPlaceOnGround()
              .then(() =>
                change({
                  ...draft,
                  behavior: { ...draft.behavior, roamingEnabled: true },
                }),
              )
              .catch((error) => setSaveError(String(error)))
              .finally(() => setSaving(false));
          }}
        >
          {zh ? "放到底部并开启漫游" : "Place at bottom and enable roaming"}
        </button>
      )}
      <label className="desktop-pet-preference-row pet-detail-interval">
        <span>
          <strong>{zh ? "自动动作间隔" : "Activity interval"}</strong>
          <small>
            {draft.behavior.quietMode
              ? zh
                ? "安静模式已开启，自动动作暂停。"
                : "Quiet mode pauses automatic activity."
              : zh
                ? "两次自动动作之间的等待时间。"
                : "Wait time between automatic actions."}
          </small>
        </span>
        <select
          aria-label={zh ? "自动动作间隔" : "Activity interval"}
          value={draft.behavior.activityIntervalSecs}
          disabled={unavailable || draft.behavior.quietMode}
          onChange={(e) =>
            change({
              ...draft,
              behavior: {
                ...draft.behavior,
                activityIntervalSecs: Number(e.currentTarget.value),
              },
            })
          }
        >
          {[...new Set([20, 45, 90, draft.behavior.activityIntervalSecs])]
            .sort((a, b) => a - b)
            .map((seconds) => (
              <option key={seconds} value={seconds}>
                {seconds} {zh ? "秒" : "seconds"}
              </option>
            ))}
        </select>
      </label>
      <div className="pet-detail-placement">
        <p className="desktop-pet-model">
          {draft.behavior.position
            ? zh
              ? "已保存位置；应用时会恢复到对应屏幕。"
              : "Saved placement restores when applied."
            : zh
              ? "使用默认位置。"
              : "Use default placement."}
        </p>
        <button
          type="button"
          className="pet-detail-clear-position"
          disabled={unavailable || !draft.behavior.position}
          onClick={() =>
            change({
              ...draft,
              behavior: { ...draft.behavior, position: null },
            })
          }
        >
          {zh ? "清除保存的位置" : "Clear saved placement"}
        </button>
      </div>
      {saveError && (
        <p className="desktop-pet-error" role="alert">
          {saveError}
        </p>
      )}
      <div className="pet-detail-save-row">
        <span role="status">
          {saving
            ? zh
              ? "正在保存…"
              : "Saving…"
            : dirty
              ? zh
                ? "有未保存的更改"
                : "Unsaved changes"
              : zh
                ? "当前配置已保存"
                : "Preferences saved"}
        </span>
        <button type="submit" disabled={unavailable || !dirty}>
          {zh ? "保存配置" : "Save preferences"}
        </button>
      </div>
    </form>
  );
}
