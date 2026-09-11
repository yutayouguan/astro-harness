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
}: {
  value: PetDefaults;
  zh: boolean;
  disabled: boolean;
  onSave: (value: PetDefaults) => Promise<unknown>;
}) {
  const [draft, setDraft] = useState(value);
  const fieldId = useId();
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState("");
  const signature = JSON.stringify(value);
  const dirty = JSON.stringify(draft) !== signature;
  const unavailable = disabled || saving;
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
          onChange={(e) =>
            change({ ...draft, scale: Number(e.currentTarget.value) })
          }
        />
        <output>{petScalePercent(draft.scale)}%</output>
      </label>
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
            disabled={unavailable}
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
