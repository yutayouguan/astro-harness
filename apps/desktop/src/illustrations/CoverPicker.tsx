/** Agent 封面缩略图选择器。 */
import { AGENT_COVERS, type CoverId } from "./registry";

type Props = {
  value: CoverId | null;
  onChange: (id: CoverId) => void;
  busy?: boolean;
  className?: string;
};

export default function CoverPicker({
  value,
  onChange,
  busy,
  className = "",
}: Props) {
  return (
    <div
      className={`astro-cover-picker ${className}`.trim()}
      role="listbox"
      aria-label="Agent cover"
    >
      {AGENT_COVERS.map((cover) => {
        const selected = value === cover.id;
        const { Art } = cover;
        return (
          <button
            key={cover.id}
            type="button"
            role="option"
            aria-selected={selected}
            disabled={busy}
            className={`astro-cover-swatch ${selected ? "is-selected" : ""}`}
            data-tone={cover.tone}
            title={cover.labelZh}
            onClick={() => onChange(cover.id)}
          >
            <span className="astro-cover-swatch-art" aria-hidden>
              <Art />
            </span>
            <span className="astro-cover-swatch-label">{cover.labelZh}</span>
          </button>
        );
      })}
    </div>
  );
}
