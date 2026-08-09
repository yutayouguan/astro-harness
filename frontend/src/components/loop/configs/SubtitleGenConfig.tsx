import { SelectField, FilePathField, cfgStr } from "./ConfigField";
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
  { value: "srt", label: "srt" },
  { value: "vtt", label: "vtt" },
  { value: "json", label: "json" },
];

const LANGUAGE_OPTIONS = [
  { value: "", label: "自动检测" },
  { value: "zh", label: "中文" },
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
];

const TRANSLATE_LANGUAGE_OPTIONS = [
  { value: "", label: "不翻译" },
  { value: "zh", label: "中文" },
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
];

export default function SubtitleGenConfig({ config, onChange, upstreamOutputs }: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <FilePathField
        label="音频/视频来源"
        value={cfgStr(config, "audio_source")}
        onChange={(v) => onChange({ ...config, audio_source: v })}
        accept="audio"
        upstream={up}
        hint="选择音频/视频文件或引用上游节点输出"
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
        mediaType="subtitle"
      />
      <SelectField
        label="语言 (可选)"
        value={cfgStr(config, "language")}
        onChange={(v) => onChange({ ...config, language: v })}
        options={LANGUAGE_OPTIONS}
      />
      <SelectField
        label="字幕格式"
        value={cfgStr(config, "output_format", "srt")}
        onChange={(v) => onChange({ ...config, output_format: v })}
        options={OUTPUT_FORMAT_OPTIONS}
      />
      <SelectField
        label="翻译目标语言 (可选)"
        value={cfgStr(config, "translate_to")}
        onChange={(v) => onChange({ ...config, translate_to: v })}
        options={TRANSLATE_LANGUAGE_OPTIONS}
      />
    </>
  );
}
