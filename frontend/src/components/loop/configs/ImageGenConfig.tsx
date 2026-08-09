import { TextField, NumberField, SelectField, cfgStr, cfgNum } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const MODE_OPTIONS = [
  { value: "text_to_image", label: "文生图" },
  { value: "image_to_image", label: "图生图 (风格/变体)" },
  { value: "face_swap", label: "换脸" },
];

const SIZE_OPTIONS = [
  { value: "1024x1024", label: "1024×1024" },
  { value: "1792x1024", label: "1792×1024" },
  { value: "1024x1792", label: "1024×1792" },
  { value: "512x512", label: "512×512" },
];

const STYLE_OPTIONS = [
  { value: "natural", label: "natural" },
  { value: "vivid", label: "vivid" },
];

const OUTPUT_FORMAT_OPTIONS = [
  { value: "png", label: "PNG" },
  { value: "webp", label: "WebP" },
  { value: "jpg", label: "JPG" },
];

export default function ImageGenConfig({ config, onChange }: ConfigProps) {
  const mode = cfgStr(config, "mode", "text_to_image");
  return (
    <>
      <SelectField
        label="生成模式"
        value={mode}
        onChange={(v) => onChange({ ...config, mode: v })}
        options={MODE_OPTIONS}
      />
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
        mediaType="image"
      />
      {(mode === "image_to_image" || mode === "face_swap") && (
        <TextField
          label="源图片"
          value={cfgStr(config, "source_image")}
          onChange={(v) => onChange({ ...config, source_image: v })}
          placeholder="图片路径或 {{var}}"
          hint="要转换/换脸的原始图片"
        />
      )}
      <TextField
        label="参考图片"
        value={cfgStr(config, "reference_images")}
        onChange={(v) => onChange({ ...config, reference_images: v })}
        multiline
        placeholder={'["ref_1.jpg", "{{node.image}}"]'}
        hint="JSON 数组，风格参考或人脸参考图"
      />
      {(mode === "image_to_image" || mode === "face_swap") && (
        <NumberField
          label="相似度强度"
          value={cfgNum(config, "strength")}
          onChange={(v) => onChange({ ...config, strength: v })}
          min={0}
          max={1}
          step={0.05}
          placeholder="0.8"
        />
      )}
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
