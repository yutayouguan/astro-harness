import { useState } from "react";
import { Fan } from "lucide-react";

type Props = {
  label: string;
  onReshuffle: () => void;
};

/** 灵动色彩模式在主聊天页的轻量重配色入口。 */
export default function DynamicPaletteButton({ label, onReshuffle }: Props) {
  const [spinRevision, setSpinRevision] = useState(0);

  const reshuffle = () => {
    setSpinRevision((revision) => revision + 1);
    onReshuffle();
  };

  return (
    <button
      type="button"
      className="shell-dynamic-palette-button"
      onClick={reshuffle}
      title={label}
      aria-label={label}
    >
      <span
        key={spinRevision}
        className="shell-dynamic-palette-icon"
        data-spinning={spinRevision > 0 || undefined}
        aria-hidden
      >
        <Fan width={18} height={18} strokeWidth={1.8} />
      </span>
    </button>
  );
}
