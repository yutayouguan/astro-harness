import { useEffect, useRef, useState } from "react";
import type { WallpaperDisplay } from "../../lib/ui/wallpaperDisplay";

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
  current.current = value;
  const pending = useRef(false);
  const [saving, setSaving] = useState(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    draftRef.current = current.current;
    setDraft(current.current);
  }, [value.fit, value.shade, value.blur]);
  function update(patch: Partial<WallpaperDisplay>) {
    draftRef.current = { ...draftRef.current, ...patch };
    setDraft(draftRef.current);
  }
  async function commit<K extends keyof WallpaperDisplay>(
    field: K,
    next: WallpaperDisplay[K],
  ) {
    if (!path || disabled || pending.current || next === current.current[field])
      return;
    pending.current = true;
    setSaving(true);
    try {
      const ok = await onCommit(path, { [field]: next });
      if (!ok && mounted.current) update(current.current);
    } catch {
      if (mounted.current) update(current.current);
    } finally {
      pending.current = false;
      if (mounted.current) setSaving(false);
    }
  }
  return (
    <details className="ambience-wallpaper-display">
      <summary>{zh ? "显示调整" : "Display adjustments"}</summary>
      <fieldset disabled={disabled || saving || !path}>
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
                onPointerDown={(e) =>
                  e.currentTarget.setPointerCapture(e.pointerId)
                }
                onPointerUp={() => void commit(field, draftRef.current[field])}
                onPointerCancel={() => update(current.current)}
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
                onBlur={() => void commit(field, draftRef.current[field])}
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
