import { TextField, SelectField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const OUTPUT_FORMAT_OPTIONS = [
  { value: "srt", label: "srt" },
  { value: "vtt", label: "vtt" },
  { value: "json", label: "json" },
];

export default function SubtitleGenConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="音频/视频来源"
        value={cfgStr(config, "audio_source")}
        onChange={(v) => onChange({ ...config, audio_source: v })}
        placeholder="{{node_id.audio_path}}"
        hint="引用上游节点输出的文件路径"
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
        placeholder="whisper-1"
      />
      <TextField
        label="语言 (可选)"
        value={cfgStr(config, "language")}
        onChange={(v) => onChange({ ...config, language: v })}
        placeholder="留空自动检测"
        hint="如 zh, en, ja"
      />
      <SelectField
        label="字幕格式"
        value={cfgStr(config, "output_format", "srt")}
        onChange={(v) => onChange({ ...config, output_format: v })}
        options={OUTPUT_FORMAT_OPTIONS}
      />
      <TextField
        label="翻译目标语言 (可选)"
        value={cfgStr(config, "translate_to")}
        onChange={(v) => onChange({ ...config, translate_to: v })}
        placeholder="en"
        hint="留空不翻译"
      />
    </>
  );
}
