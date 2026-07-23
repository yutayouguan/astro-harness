import { TextField, NumberField, SelectField, cfgStr, cfgNum } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const VOICE_OPTIONS = [
  { value: "alloy", label: "alloy" },
  { value: "echo", label: "echo" },
  { value: "fable", label: "fable" },
  { value: "onyx", label: "onyx" },
  { value: "nova", label: "nova" },
  { value: "shimmer", label: "shimmer" },
];

const OUTPUT_FORMAT_OPTIONS = [
  { value: "mp3", label: "mp3" },
  { value: "opus", label: "opus" },
  { value: "wav", label: "wav" },
  { value: "flac", label: "flac" },
];

export default function TtsConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="朗读文本"
        value={cfgStr(config, "text_template")}
        onChange={(v) => onChange({ ...config, text_template: v })}
        placeholder="输入要转语音的文字…"
        multiline
        hint="支持 {{var}} 引用上游变量"
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
        mediaType="tts"
      />
      <SelectField
        label="音色"
        value={cfgStr(config, "voice", "alloy")}
        onChange={(v) => onChange({ ...config, voice: v })}
        options={VOICE_OPTIONS}
      />
      <NumberField
        label="语速"
        value={cfgNum(config, "speed")}
        onChange={(v) => onChange({ ...config, speed: v })}
        min={0.25}
        max={4.0}
        step={0.25}
        placeholder="1.0"
      />
      <SelectField
        label="输出格式"
        value={cfgStr(config, "output_format", "mp3")}
        onChange={(v) => onChange({ ...config, output_format: v })}
        options={OUTPUT_FORMAT_OPTIONS}
      />
    </>
  );
}
