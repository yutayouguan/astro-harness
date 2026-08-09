import { AiAssistField, TextField, NumberField, SelectField, ToggleField, cfgStr, cfgNum, cfgBool } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
  aiProviderId?: string;
  aiModel?: string;
}

const GENRE_OPTIONS = [
  { value: "pop", label: "Pop" },
  { value: "jazz", label: "Jazz" },
  { value: "electronic", label: "Electronic" },
  { value: "classical", label: "Classical" },
  { value: "rock", label: "Rock" },
  { value: "ambient", label: "Ambient" },
  { value: "hiphop", label: "Hip-Hop" },
  { value: "custom", label: "自定义" },
];

export default function MusicGenConfig({ config, onChange, aiProviderId, aiModel }: ConfigProps) {
  return (
    <>
      <AiAssistField
        label="音乐描述"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="轻快的电子乐，适合产品宣传…"
        hint="描述你想要的音乐风格、情绪和用途"
        task="音乐描述"
        aiProviderId={aiProviderId}
        aiModel={aiModel}
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
        mediaType="music"
      />
      <NumberField
        label="时长 (秒)"
        value={cfgNum(config, "duration_seconds")}
        onChange={(v) => onChange({ ...config, duration_seconds: v })}
        min={5}
        max={300}
        placeholder="30"
        hint="秒"
      />
      <ToggleField
        label="纯器乐（无人声）"
        value={cfgBool(config, "instrumental")}
        onChange={(v) => onChange({ ...config, instrumental: v })}
      />
      <SelectField
        label="曲风 (可选)"
        value={cfgStr(config, "genre")}
        onChange={(v) => onChange({ ...config, genre: v })}
        options={GENRE_OPTIONS}
      />
      {cfgStr(config, "genre") === "custom" && (
        <TextField
          label="自定义曲风"
          value={cfgStr(config, "custom_genre")}
          onChange={(v) => onChange({ ...config, custom_genre: v })}
          placeholder="输入曲风名称，如 lo-fi, synthwave…"
        />
      )}
    </>
  );
}
