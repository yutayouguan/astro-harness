import { TextField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function SetFieldsConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="字段赋值 (JSON)"
        value={cfgStr(config, "assignments")}
        onChange={(v) => onChange({ ...config, assignments: v })}
        multiline
        placeholder={'[\n  {"field": "name", "value": "{{input.name}}"},\n  {"field": "count", "value": 42}\n]'}
        hint="每条赋值设定一个输出字段"
      />
    </>
  );
}
