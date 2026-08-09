import { TextField, SelectField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const LANGUAGE_OPTIONS = [
  { value: "auto", label: "自动检测" },
  { value: "zh", label: "中文" },
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
];

const OUTPUT_OPTIONS = [
  { value: "text", label: "纯文本" },
  { value: "srt", label: "SRT 字幕" },
  { value: "vtt", label: "VTT 字幕" },
  { value: "json", label: "JSON (含时间戳)" },
];

export default function SpeechToTextConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="音频/视频源"
        value={cfgStr(config, "input_path")}
        onChange={(v) => onChange({ ...config, input_path: v })}
        placeholder="文件路径或 {{var}}"
        hint="支持 mp3/wav/mp4 等常见格式"
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
      />
      <SelectField
        label="语言"
        value={cfgStr(config, "language", "auto")}
        onChange={(v) => onChange({ ...config, language: v })}
        options={LANGUAGE_OPTIONS}
      />
      <SelectField
        label="输出格式"
        value={cfgStr(config, "output_format", "text")}
        onChange={(v) => onChange({ ...config, output_format: v })}
        options={OUTPUT_OPTIONS}
      />
      <TextField
        label="初始提示"
        value={cfgStr(config, "prompt")}
        onChange={(v) => onChange({ ...config, prompt: v })}
        placeholder="可选，引导模型识别特定术语"
      />
    </>
  );
}
