import { useEffect, useId, useRef, useState } from "react";
import {
  DESKTOP_PET_SCALE,
  petScalePercent,
} from "../../lib/ui/desktopPetState";

/** Draft locally; one native resize on release, keyboard completion or blur. */
export default function AmbiencePetScaleControl({
  petId,
  name,
  scale,
  disabled,
  zh,
  onCommit,
}: {
  petId: string | null;
  name: string | null;
  scale: number;
  disabled: boolean;
  zh: boolean;
  onCommit: (petId: string, scale: number) => Promise<boolean>;
}) {
  const id = useId();
  const [draft, setDraft] = useState(scale);
  const draftRef = useRef(scale);
  const latestScale = useRef(scale);
  latestScale.current = scale;
  const pending = useRef(false);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState("");
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    draftRef.current = scale;
    setDraft(scale);
  }, [scale]);
  const changeDraft = (value: number) => {
    draftRef.current = value;
    setDraft(value);
  };
  const commit = async () => {
    if (
      !petId ||
      disabled ||
      pending.current ||
      draftRef.current === latestScale.current
    )
      return;
    pending.current = true;
    setSaving(true);
    setStatus("");
    try {
      const ok = await onCommit(petId, draftRef.current);
      if (mounted.current) {
        if (!ok) changeDraft(latestScale.current);
        setStatus(
          ok
            ? zh
              ? "桌宠大小已更新"
              : "Pet size updated"
            : zh
              ? "未完成大小调整"
              : "Resize did not complete",
        );
      }
    } catch {
      if (mounted.current) {
        changeDraft(latestScale.current);
        setStatus(zh ? "未完成大小调整" : "Resize did not complete");
      }
    } finally {
      pending.current = false;
      if (mounted.current) setSaving(false);
    }
  };
  return (
    <div className="ambience-pet-scale">
      <label className="ambience-strength" htmlFor={id}>
        <span>
          {zh ? "桌宠大小" : "Pet size"}
          <output>{petScalePercent(draft)}%</output>
        </span>
        <input
          id={id}
          type="range"
          min={DESKTOP_PET_SCALE.min}
          max={DESKTOP_PET_SCALE.max}
          step={DESKTOP_PET_SCALE.step}
          value={draft}
          disabled={disabled || saving || !petId}
          aria-label={zh ? "桌宠大小" : "Pet size"}
          aria-valuetext={petScalePercent(draft) + "%"}
          aria-describedby={id + "-hint"}
          onChange={(event) => changeDraft(Number(event.currentTarget.value))}
          onPointerDown={(event) =>
            event.currentTarget.setPointerCapture(event.pointerId)
          }
          onPointerUp={() => void commit()}
          onPointerCancel={() => changeDraft(latestScale.current)}
          onKeyUp={(event) => {
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
              ].includes(event.key)
            )
              void commit();
          }}
          onBlur={() => void commit()}
        />
      </label>
      <p className="ambience-help" id={id + "-hint"}>
        {petId
          ? (zh ? "松手应用到当前桌宠：" : "Applies on release to: ") +
            (name ?? petId)
          : zh
            ? "请先选择桌宠"
            : "Select a desktop pet first"}
      </p>
      <span className="sr-only" role="status">
        {saving ? (zh ? "正在调整大小…" : "Resizing…") : status}
      </span>
    </div>
  );
}
