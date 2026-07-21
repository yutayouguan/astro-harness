import { TextField, NumberField, SelectField, cfgStr, cfgNum } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const ASPECT_RATIO_OPTIONS = [
  { value: "16:9", label: "16:9" },
  { value: "9:16", label: "9:16" },
  { value: "1:1", label: "1:1" },
  { value: "4:3", label: "4:3" },
];

export default function VideoGenConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="生成提示词"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="描述你要生成的视频…"
        multiline
      />
      <TextField
        label="供应商 (可选)"
        value={cfgStr(config, "provider_id")}
        onChange={(v) => onChange({ ...config, provider_id: v })}
      />
      <TextField
        label="模型"
        value={cfgStr(config, "model")}
        onChange={(v) => onChange({ ...config, model: v })}
        placeholder="runway-gen3 / kling-v2 / minimax-video"
      />
      <NumberField
        label="时长 (秒)"
        value={cfgNum(config, "duration_seconds")}
        onChange={(v) => onChange({ ...config, duration_seconds: v })}
        min={1}
        max={60}
        placeholder="5"
      />
      <SelectField
        label="画面比例"
        value={cfgStr(config, "aspect_ratio", "16:9")}
        onChange={(v) => onChange({ ...config, aspect_ratio: v })}
        options={ASPECT_RATIO_OPTIONS}
      />
      <TextField
        label="参考图片 (可选)"
        value={cfgStr(config, "reference_image")}
        onChange={(v) => onChange({ ...config, reference_image: v })}
        placeholder="{{node_id.image_path}}"
        hint="可引用上游图片节点的输出路径"
      />
    </>
  );
}
