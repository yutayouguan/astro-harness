import { TextField, SelectField, FilePathField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
  aiProviderId?: string;
  aiModel?: string;
}

const OUTPUT_FORMAT_OPTIONS = [
  { value: "mp3", label: "MP3" },
  { value: "wav", label: "WAV" },
  { value: "opus", label: "Opus" },
];

export default function VoiceCloneConfig({
  config,
  onChange,
  upstreamOutputs,
}: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <FilePathField
        label="参考音频"
        value={cfgStr(config, "reference_audio")}
        onChange={(v) => onChange({ ...config, reference_audio: v })}
        accept="audio"
        upstream={up}
        hint="用于克隆的目标音色样本（建议 10-30 秒清晰人声）"
      />
      <TextField
        label="合成文本"
        value={cfgStr(config, "text_template")}
        onChange={(v) => onChange({ ...config, text_template: v })}
        placeholder="输入要合成的文字…"
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
      <TextField
        label="说话人 ID"
        value={cfgStr(config, "speaker_id")}
        onChange={(v) => onChange({ ...config, speaker_id: v })}
        placeholder="可选，复用已克隆的说话人"
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
