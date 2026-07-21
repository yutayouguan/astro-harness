import { TextField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function FilterConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="过滤条件"
        value={cfgStr(config, "condition")}
        onChange={(v) => onChange({ ...config, condition: v })}
        placeholder={'{{status}} == "active"'}
        hint="条件为 true 时放行，否则阻断"
      />
    </>
  );
}
