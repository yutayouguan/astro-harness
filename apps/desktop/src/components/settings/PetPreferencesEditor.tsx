import { useEffect, useState } from "react";
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
  const [dirty, setDirty] = useState(false);
  const signature = JSON.stringify(value);
  useEffect(() => {
    setDraft(JSON.parse(signature) as PetDefaults);
    setDirty(false);
  }, [signature]);
  const change = (next: PetDefaults) => {
    setDraft(next);
    setDirty(true);
  };
  return (
    <form
      className="pet-defaults-editor"
      onSubmit={(event) => {
        event.preventDefault();
        void onSave(draft);
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
          disabled={disabled}
          aria-valuetext={`${petScalePercent(draft.scale)}%`}
          onChange={(e) =>
            change({ ...draft, scale: Number(e.currentTarget.value) })
          }
        />
        <output>{petScalePercent(draft.scale)}%</output>
      </label>
      {(
        [
          ["positionLocked", zh ? "锁定位置" : "Lock position"],
          ["snapToEdge", zh ? "贴边吸附" : "Snap to edges"],
          ["quietMode", zh ? "安静模式" : "Quiet mode"],
        ] as const
      ).map(([key, label]) => (
        <label className="desktop-pet-preference-row" key={key}>
          <span>{label}</span>
          <input
            type="checkbox"
            checked={draft.behavior[key]}
            disabled={disabled}
            onChange={(e) =>
              change({
                ...draft,
                behavior: { ...draft.behavior, [key]: e.currentTarget.checked },
              })
            }
          />
        </label>
      ))}
      <label className="desktop-pet-preference-row">
        <span>{zh ? "自动动作间隔" : "Activity interval"}</span>
        <select
          value={draft.behavior.activityIntervalSecs}
          disabled={disabled || draft.behavior.quietMode}
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
      <p className="desktop-pet-model">
        {draft.behavior.position
          ? zh
            ? "已保存位置；应用时会恢复到对应屏幕。"
            : "Saved placement restores when applied."
          : zh
            ? "使用默认位置。"
            : "Use default placement."}
      </p>
      <div className="pet-scene-actions">
        <button type="submit" disabled={disabled || !dirty}>
          {zh ? "保存配置" : "Save preferences"}
        </button>
        <button
          type="button"
          disabled={disabled || !draft.behavior.position}
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
    </form>
  );
}
