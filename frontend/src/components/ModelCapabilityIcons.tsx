/** ModelPicker 行内能力图标。 */
import {
  AudioLines,
  Brain,
  Eye,
  Globe,
  Image,
  Video,
  Wrench,
} from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import {
  listActiveModelCaps,
  type ModelCapKey,
} from "../lib/modelCaps";
import type { ModelCapabilities } from "../types";

const CAP_META: Record<
  ModelCapKey,
  { Icon: typeof Wrench; labelKey: MessageKey }
> = {
  tools: { Icon: Wrench, labelKey: "modelCaps.tools" },
  reasoning: { Icon: Brain, labelKey: "modelCaps.reasoning" },
  vision: { Icon: Eye, labelKey: "modelCaps.vision" },
  web: { Icon: Globe, labelKey: "modelCaps.web" },
  image_gen: { Icon: Image, labelKey: "modelCaps.imageGen" },
  video_gen: { Icon: Video, labelKey: "modelCaps.videoGen" },
  audio_gen: { Icon: AudioLines, labelKey: "modelCaps.audioGen" },
};

type Props = {
  capabilities?: ModelCapabilities | null;
  className?: string;
};

/** 按固定顺序渲染为 true 的能力小图标；全无则不渲染。 */
export default function ModelCapabilityIcons({
  capabilities,
  className = "",
}: Props) {
  const { t } = useI18n();
  const active = listActiveModelCaps(capabilities);
  if (active.length === 0) return null;

  return (
    <span className={`model-picker-cap-icons ${className}`.trim()} aria-hidden={false}>
      {active.map((key) => {
        const { Icon, labelKey } = CAP_META[key];
        const label = t(labelKey);
        return (
          <span
            key={key}
            className="model-picker-cap-icon"
            title={label}
            aria-label={label}
          >
            <Icon size={12} strokeWidth={2} aria-hidden />
          </span>
        );
      })}
    </span>
  );
}
