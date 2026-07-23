import { TextField, NumberField, SelectField, cfgStr, cfgNum } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const SIZE_OPTIONS = [
  { value: "1024x1024", label: "1024x1024" },
  { value: "1792x1024", label: "1792x1024" },
  { value: "1024x1792", label: "1024x1792" },
  { value: "512x512", label: "512x512" },
];

const STYLE_OPTIONS = [
  { value: "natural", label: "natural" },
  { value: "vivid", label: "vivid" },
];

const OUTPUT_FORMAT_OPTIONS = [
  { value: "png", label: "png" },
  { value: "webp", label: "webp" },
  { value: "b64_json", label: "b64_json" },
];

export default function ImageGenConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="生成提示词"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="描述你要生成的图片…"
        multiline
        hint="支持 {{var}} 引用上游变量"
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
      />
      <SelectField
        label="尺寸"
        value={cfgStr(config, "size", "1024x1024")}
        onChange={(v) => onChange({ ...config, size: v })}
        options={SIZE_OPTIONS}
      />
      <SelectField
        label="风格"
        value={cfgStr(config, "style", "natural")}
        onChange={(v) => onChange({ ...config, style: v })}
        options={STYLE_OPTIONS}
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
