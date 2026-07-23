import { TextField, NumberField, ToggleField, cfgStr, cfgNum, cfgBool } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function MusicGenConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="音乐描述"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="轻快的电子乐，适合产品宣传…"
        multiline
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
      />
      <NumberField
        label="时长 (秒)"
        value={cfgNum(config, "duration_seconds")}
        onChange={(v) => onChange({ ...config, duration_seconds: v })}
        min={5}
        max={300}
        placeholder="30"
      />
      <ToggleField
        label="纯器乐（无人声）"
        value={cfgBool(config, "instrumental")}
        onChange={(v) => onChange({ ...config, instrumental: v })}
      />
      <TextField
        label="曲风 (可选)"
        value={cfgStr(config, "genre")}
        onChange={(v) => onChange({ ...config, genre: v })}
        placeholder="pop / jazz / electronic"
      />
    </>
  );
}
