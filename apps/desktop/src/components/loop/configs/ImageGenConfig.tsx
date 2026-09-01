import {
  AiAssistField,
  NumberField,
  SelectField,
  cfgStr,
  cfgNum,
} from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
  aiProviderId?: string;
  aiModel?: string;
}

const SIZE_OPTIONS = [
  { value: "1024x1024", label: "1024×1024" },
  { value: "1536x1024", label: "1536×1024" },
  { value: "1024x1536", label: "1024×1536" },
];

const OUTPUT_FORMAT_OPTIONS = [
  { value: "png", label: "PNG" },
  { value: "webp", label: "WebP" },
  { value: "jpeg", label: "JPEG" },
];

export default function ImageGenConfig({
  config,
  onChange,
  upstreamOutputs,
  aiProviderId,
  aiModel,
}: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <AiAssistField
        label="生成提示词"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="描述你要生成的图片…"
        hint="支持 {{var}} 引用上游变量"
        task="图片生成提示词"
        aiProviderId={aiProviderId}
        aiModel={aiModel}
        upstream={up}
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
        mediaType="image"
      />
      <SelectField
        label="尺寸"
        value={cfgStr(config, "size", "1024x1024")}
        onChange={(v) => onChange({ ...config, size: v })}
        options={SIZE_OPTIONS}
      />
      <NumberField
        label="生成张数"
        value={cfgNum(config, "num_images")}
        onChange={(v) => onChange({ ...config, num_images: v })}
        min={1}
        max={10}
        placeholder="1"
      />
      <SelectField
        label="输出格式"
        value={cfgStr(config, "output_format", "png")}
        onChange={(v) => onChange({ ...config, output_format: v })}
        options={OUTPUT_FORMAT_OPTIONS}
      />
    </>
  );
}
