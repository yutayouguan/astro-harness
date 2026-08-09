import { useMemo } from "react";
import { TextField, NumberField, SelectField, cfgStr, cfgNum } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import { getVoiceOptions } from "./minimaxVoices";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
}

const OUTPUT_FORMAT_OPTIONS = [
  { value: "mp3", label: "MP3" },
  { value: "opus", label: "Opus" },
  { value: "wav", label: "WAV" },
  { value: "flac", label: "FLAC" },
];

export default function TtsConfig({ config, onChange, upstreamOutputs }: ConfigProps) {
  const up = upstreamOutputs ?? [];
  const providerId = cfgStr(config, "provider_id");
  const voices = useMemo(() => getVoiceOptions(providerId), [providerId]);
  const currentVoice = cfgStr(config, "voice");
  const hasVoiceList = voices.length > 0;

  return (
    <>
      <TextField
        label="朗读文本"
        value={cfgStr(config, "text_template")}
        onChange={(v) => onChange({ ...config, text_template: v })}
        placeholder="输入要转语音的文字…"
        multiline
        hint="支持 {{var}} 引用上游变量"
        upstream={up}
      />
      <ProviderModelSelect
        providerId={providerId}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
        mediaType="tts"
      />
      {hasVoiceList ? (
        <SelectField
          label="音色"
          value={currentVoice}
          onChange={(v) => onChange({ ...config, voice: v })}
          options={[
            { value: "", label: "选择音色…" },
            ...voices.map((v) => ({
              value: v.value,
              label: v.group ? `${v.group} / ${v.label}` : v.label,
            })),
            ...(currentVoice && !voices.some((v) => v.value === currentVoice)
              ? [{ value: currentVoice, label: `${currentVoice} (自定义)` }]
              : []),
          ]}
          hint="也可在下方手动输入自定义 voice_id（如克隆音色）"
        />
      ) : (
        <TextField
          label="音色 (Voice ID)"
          value={currentVoice}
          onChange={(v) => onChange({ ...config, voice: v })}
          placeholder="输入 voice_id"
          hint="当前供应商无预置音色列表，请手动输入"
        />
      )}
      {hasVoiceList && (
        <TextField
          label="自定义 Voice ID（可选）"
          value={currentVoice && !voices.some((v) => v.value === currentVoice) ? currentVoice : ""}
          onChange={(v) => onChange({ ...config, voice: v })}
          placeholder="手动输入 voice_id（覆盖上方选择）"
          hint="如已克隆的音色 ID"
        />
      )}
      <NumberField
        label="语速"
        value={cfgNum(config, "speed")}
        onChange={(v) => onChange({ ...config, speed: v })}
        min={0.25}
        max={4.0}
        step={0.25}
        placeholder="1.0"
        hint="0.25 - 4.0"
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
