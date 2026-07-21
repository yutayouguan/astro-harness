import { TextField, NumberField, ToggleField, cfgStr, cfgNum, cfgBool } from "./ConfigField";

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
      <TextField
        label="供应商 (可选)"
        value={cfgStr(config, "provider_id")}
        onChange={(v) => onChange({ ...config, provider_id: v })}
      />
      <TextField
        label="模型"
        value={cfgStr(config, "model")}
        onChange={(v) => onChange({ ...config, model: v })}
        placeholder="suno-v4 / minimax-music"
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
