import { useEffect, useId, useRef, useState, type CSSProperties } from "react";
import {
  DESKTOP_PET_SCALE,
  petScalePercent,
} from "../../lib/ui/desktopPetState";
import { createLivePetScaleQueue } from "../../lib/ui/livePetScale";

/** Stream live native size updates while leaving thumb dragging to the WebView. */
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
  onCommit: (
    petId: string,
    scale: number,
    gestureId: string,
  ) => Promise<boolean>;
}) {
  const id = useId();
  const [draft, setDraft] = useState(scale);
  const latestScale = useRef(scale);
  latestScale.current = scale;
  const mounted = useRef(true);
  const commitRef = useRef(onCommit);
  commitRef.current = onCommit;
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState("");
  const serial = useRef(0);
  const gesture = useRef<{ id: string; pointerId: number | null } | null>(null);
  const [queue] = useState(() =>
    createLivePetScaleQueue({
      apply: (request) =>
        commitRef.current(request.petId, request.scale, request.gestureId),
      busy: (busy) => {
        if (mounted.current) setSaving(busy);
      },
      settled: (ok) => {
        if (!mounted.current) return;
        if (!ok) {
          gesture.current = null;
          setDraft(latestScale.current);
        }
        setStatus(
          ok
            ? zh
              ? "桌宠大小已更新"
              : "Pet size updated"
            : zh
              ? "未完成大小调整"
              : "Resize did not complete",
        );
      },
    }),
  );
  useEffect(() => {
    mounted.current = true;
    // Finish already requested values after closing; the native petId guard
    // prevents an old queued gesture from resizing a different current pet.
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    if (!queue.isPending() && !gesture.current) setDraft(scale);
  }, [scale, saving, queue]);
  useEffect(() => {
    const finish = (event: PointerEvent) => {
      if (gesture.current?.pointerId === event.pointerId) {
        gesture.current = null;
        if (!queue.isPending()) setDraft(latestScale.current);
      }
    };
    const blur = () => {
      gesture.current = null;
      if (!queue.isPending()) setDraft(latestScale.current);
    };
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      window.removeEventListener("blur", blur);
    };
  }, []);
  const startGesture = (pointerId: number | null) => {
    gesture.current = { id: id + "-" + ++serial.current, pointerId };
  };
  const change = (value: number) => {
    if (!petId || (disabled && !queue.isPending())) return;
    setDraft(value);
    setStatus("");
    const gestureId = gesture.current?.id ?? id + "-" + ++serial.current;
    void queue.enqueue({ petId, scale: value, gestureId });
  };
  return (
    <div className="ambience-pet-scale" aria-busy={saving}>
      <label className="ambience-strength" htmlFor={id}>
        <span>
          <span className="ambience-size-label" title={name ?? undefined}>
            {zh ? "桌宠大小" : "Pet size"}
            {petId && <small> · {name ?? petId}</small>}
          </span>
          <output>{petScalePercent(draft)}%</output>
        </span>
        <input
          id={id}
          className="ambience-range"
          type="range"
          min={DESKTOP_PET_SCALE.min}
          max={DESKTOP_PET_SCALE.max}
          step={DESKTOP_PET_SCALE.step}
          style={
            {
              "--range-progress":
                ((draft - DESKTOP_PET_SCALE.min) /
                  (DESKTOP_PET_SCALE.max - DESKTOP_PET_SCALE.min)) *
                  100 +
                "%",
            } as CSSProperties
          }
          value={draft}
          disabled={!petId || (disabled && !saving)}
          aria-label={zh ? "桌宠大小" : "Pet size"}
          aria-valuetext={petScalePercent(draft) + "%"}
          aria-describedby={id + "-hint"}
          onChange={(event) => change(Number(event.currentTarget.value))}
          onPointerDown={(event) => {
            if (event.isPrimary && event.button === 0)
              startGesture(event.pointerId);
          }}
          onKeyDown={(event) => {
            if (
              !event.repeat &&
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
              startGesture(null);
          }}
          onKeyUp={() => {
            if (gesture.current?.pointerId === null) gesture.current = null;
          }}
          onBlur={() => {
            if (gesture.current?.pointerId === null) gesture.current = null;
          }}
        />
      </label>
      <p className={petId ? "sr-only" : "ambience-help"} id={id + "-hint"}>
        {petId
          ? (zh ? "实时调整当前桌宠：" : "Live size for: ") + (name ?? petId)
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
