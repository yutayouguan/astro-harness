import { TextField, NumberField, SelectField, cfgStr, cfgNum } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const MODE_OPTIONS = [
  { value: "text_to_video", label: "文生视频" },
  { value: "image_to_video", label: "图生视频" },
  { value: "video_to_video", label: "视频转视频 (换脸/风格)" },
];

const ASPECT_RATIO_OPTIONS = [
  { value: "16:9", label: "16:9" },
  { value: "9:16", label: "9:16" },
  { value: "1:1", label: "1:1" },
  { value: "4:3", label: "4:3" },
];

export default function VideoGenConfig({ config, onChange }: ConfigProps) {
  const mode = cfgStr(config, "mode", "text_to_video");
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
        placeholder="描述你要生成的视频…"
        multiline
        hint="支持 {{var}} 引用上游变量"
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
        mediaType="video"
      />
      {mode === "video_to_video" && (
        <TextField
          label="源视频"
          value={cfgStr(config, "source_video")}
          onChange={(v) => onChange({ ...config, source_video: v })}
          placeholder="视频文件路径或 {{var}}"
          hint="要替换/转换的原始视频"
        />
      )}
      <TextField
        label="参考图片"
        value={cfgStr(config, "reference_images")}
        onChange={(v) => onChange({ ...config, reference_images: v })}
        multiline
        placeholder={'["face_1.jpg", "face_2.jpg", "{{node.image}}"]'}
        hint="JSON 数组，支持多张参考图和 {{var}} 引用"
      />
      <NumberField
        label="时长 (秒)"
        value={cfgNum(config, "duration_seconds")}
        onChange={(v) => onChange({ ...config, duration_seconds: v })}
        min={1}
        max={300}
        placeholder="5"
      />
      <SelectField
        label="画面比例"
        value={cfgStr(config, "aspect_ratio", "16:9")}
        onChange={(v) => onChange({ ...config, aspect_ratio: v })}
        options={ASPECT_RATIO_OPTIONS}
      />
      <NumberField
        label="相似度强度"
        value={cfgNum(config, "strength")}
        onChange={(v) => onChange({ ...config, strength: v })}
        min={0}
        max={1}
        step={0.05}
        placeholder="0.8"
      />
    </>
  );
}
