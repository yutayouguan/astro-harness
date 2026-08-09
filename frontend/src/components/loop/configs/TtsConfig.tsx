import { useMemo } from "react";
import { TextField, NumberField, SelectField, cfgStr, cfgNum } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import { getVoiceOptions, getVoiceGroups } from "./minimaxVoices";
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
  { value: "opus", label: "Opus" },
  { value: "wav", label: "WAV" },
  { value: "flac", label: "FLAC" },
];

export default function TtsConfig({ config, onChange }: ConfigProps) {
  const providerId = cfgStr(config, "provider_id");
  const voices = useMemo(() => getVoiceOptions(providerId), [providerId]);
  const groups = useMemo(() => getVoiceGroups(voices), [voices]);
  const currentVoice = cfgStr(config, "voice");

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
        providerId={providerId}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
        mediaType="tts"
      />
      <label className="loop-config-field">
        <span className="loop-config-label">音色</span>
        <select
          className="loop-config-select"
          value={currentVoice}
          onChange={(e) => onChange({ ...config, voice: e.target.value })}
        >
          <option value="">选择音色…</option>
          {groups.map((g) => (
            <optgroup key={g} label={g}>
              {voices
                .filter((v) => v.group === g)
                .map((v) => (
                  <option key={v.value} value={v.value}>
                    {v.label}
                  </option>
                ))}
            </optgroup>
          ))}
          {currentVoice && !voices.some((v) => v.value === currentVoice) && (
            <option value={currentVoice}>{currentVoice}</option>
          )}
        </select>
        <span className="loop-config-hint">
          也可直接输入自定义 voice_id（如克隆音色）
        </span>
        <input
          className="loop-config-input"
          value={currentVoice}
          onChange={(e) => onChange({ ...config, voice: e.target.value })}
          placeholder="或手动输入 voice_id"
          style={{ marginTop: 4 }}
        />
      </label>
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
