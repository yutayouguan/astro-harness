import { TextField, SelectField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const OPERATION_OPTIONS = [
  { value: "sum", label: "求和 (Sum)" },
  { value: "count", label: "计数 (Count)" },
  { value: "average", label: "平均 (Average)" },
  { value: "min", label: "最小值 (Min)" },
  { value: "max", label: "最大值 (Max)" },
  { value: "concat", label: "拼接 (Concat)" },
];

export default function AggregateConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <SelectField
        label="聚合操作"
        value={cfgStr(config, "operation", "sum")}
        onChange={(v) => onChange({ ...config, operation: v })}
        options={OPERATION_OPTIONS}
      />
      <TextField
        label="目标字段"
        value={cfgStr(config, "field")}
        onChange={(v) => onChange({ ...config, field: v })}
        placeholder="amount"
        hint="对数组中每个元素的该字段执行聚合"
      />
    </>
  );
}
