import { TextField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function OutputConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="输出字段 (JSON)"
        value={cfgStr(config, "fields")}
        onChange={(v) => onChange({ ...config, fields: v })}
        multiline
        placeholder={'[\n  {"name": "result", "field_type": "string", "description": "最终输出"}\n]'}
        hint="定义此 Loop 的最终输出字段"
      />
    </>
  );
}
