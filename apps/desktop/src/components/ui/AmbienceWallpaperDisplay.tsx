import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  createWallpaperDisplaySaver,
  type WallpaperDisplay,
} from "../../lib/ui/wallpaperDisplay";

export default function AmbienceWallpaperDisplay({
  path,
  value,
  disabled,
  zh,
  onCommit,
}: {
  path: string | null;
  value: WallpaperDisplay;
  disabled: boolean;
  zh: boolean;
  onCommit: (
    path: string,
    patch: Partial<WallpaperDisplay>,
  ) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState(value);
  const draftRef = useRef(value);
  const current = useRef(value);
  const gesture = useRef<{ field: "shade" | "blur"; pointerId: number } | null>(
    null,
  );
  const commitRef = useRef(onCommit);
  commitRef.current = onCommit;
  const [saving, setSaving] = useState(false);
  const mounted = useRef(true);
  const pathRef = useRef(path);
  pathRef.current = path;
  const [saver] = useState(() =>
    createWallpaperDisplaySaver({
      current: () => current.current,
      commit: (patch) =>
        mounted.current && pathRef.current
          ? commitRef.current(pathRef.current, patch)
          : Promise.resolve(false),
      applied: (patch) => {
        current.current = { ...current.current, ...patch };
      },
      rejected: () => {
        if (mounted.current) update(current.current);
      },
      busy: (busy) => {
        if (mounted.current) setSaving(busy);
      },
    }),
  );
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      saver.clearPending();
    };
  }, []);
  useEffect(() => {
    current.current = value;
    if (!saver.isSaving() && !gesture.current) {
      draftRef.current = value;
      setDraft(value);
    }
  }, [value.fit, value.shade, value.blur]);
  function update(patch: Partial<WallpaperDisplay>) {
    draftRef.current = { ...draftRef.current, ...patch };
    setDraft(draftRef.current);
  }
  function commit<K extends keyof WallpaperDisplay>(
    field: K,
    next: WallpaperDisplay[K],
  ) {
    if (!path || (disabled && !saver.isSaving())) return;
    return saver.enqueue({ [field]: next });
  }
  const finishPointer = useRef<(event: PointerEvent) => void>(() => {});
  finishPointer.current = (event) => {
    const active = gesture.current;
    if (!active || active.pointerId !== event.pointerId) return;
    gesture.current = null;
    if (event.type === "pointercancel") update(current.current);
    else void commit(active.field, draftRef.current[active.field]);
  };
  useEffect(() => {
    // Native range owns thumb dragging. Capturing on the host breaks WebKit's
    // internal thumb; listen for release outside the track instead.
    const finish = (event: PointerEvent) => finishPointer.current(event);
    const cancel = () => {
      if (gesture.current) {
        gesture.current = null;
        update(current.current);
      }
    };
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    window.addEventListener("blur", cancel);
    return () => {
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      window.removeEventListener("blur", cancel);
    };
  }, []);
  return (
    <details className="ambience-wallpaper-display" aria-busy={saving}>
      <summary>{zh ? "显示调整" : "Display adjustments"}</summary>
      <fieldset disabled={!path || (disabled && !saving)}>
        <legend className="sr-only">
          {zh ? "壁纸显示" : "Wallpaper display"}
        </legend>
        <div className="ambience-choice">
          <span className="ambience-display-label">
            {zh ? "填充方式" : "Image fit"}
          </span>
          <div>
            {(["cover", "contain", "stretch"] as const).map((fit) => (
              <button
                key={fit}
                type="button"
                aria-pressed={draft.fit === fit}
                onClick={() => {
                  update({ fit });
                  void commit("fit", fit);
                }}
              >
                {fit === "cover"
                  ? zh
                    ? "填满"
                    : "Fill"
                  : fit === "contain"
                    ? zh
                      ? "适应"
                      : "Fit"
                    : zh
                      ? "拉伸"
                      : "Stretch"}
              </button>
            ))}
          </div>
        </div>
        {(["shade", "blur"] as const).map((field) => {
          const label =
            field === "shade"
              ? zh
                ? "内容保护"
                : "Content protection"
              : zh
                ? "柔化背景"
                : "Background blur";
          const text =
            field === "shade"
              ? draft[field] + "%"
              : draft[field] === 0
                ? zh
                  ? "关闭"
                  : "Off"
                : draft[field] + "px";
          return (
            <label key={field} className="ambience-strength">
              <span>
                {label}
                <output>{text}</output>
              </span>
              <input
                className="ambience-wallpaper-range"
                style={
                  {
                    "--range-progress":
                      (draft[field] / (field === "shade" ? 55 : 12)) * 100 +
                      "%",
                  } as CSSProperties
                }
                type="range"
                min={0}
                max={field === "shade" ? 55 : 12}
                step={1}
                value={draft[field]}
                aria-label={label}
                aria-valuetext={text}
                onChange={(e) =>
                  update({ [field]: Number(e.currentTarget.value) })
                }
                onPointerDown={(event) => {
                  if (event.isPrimary && event.button === 0)
                    gesture.current = { field, pointerId: event.pointerId };
                }}
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
                    void commit(field, draftRef.current[field]);
                }}
                onBlur={() => {
                  if (!gesture.current)
                    void commit(field, draftRef.current[field]);
                }}
              />
            </label>
          );
        })}
      </fieldset>
      {!path && (
        <p className="ambience-help">
          {zh ? "先选择一张图片壁纸" : "Choose a wallpaper first"}
        </p>
      )}
    </details>
  );
}
