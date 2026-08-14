import { TextField, SelectField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const ORDER_OPTIONS = [
  { value: "asc", label: "升序 (ASC)" },
  { value: "desc", label: "降序 (DESC)" },
];

export default function SortConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="排序字段"
        value={cfgStr(config, "field")}
        onChange={(v) => onChange({ ...config, field: v })}
        placeholder="price"
        hint="数组元素中用于排序的字段名"
      />
      <SelectField
        label="排序方向"
        value={cfgStr(config, "order", "asc")}
        onChange={(v) => onChange({ ...config, order: v })}
        options={ORDER_OPTIONS}
      />
    </>
  );
}
