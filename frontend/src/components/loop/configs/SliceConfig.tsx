import { NumberField, cfgNum } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function SliceConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <NumberField
        label="起始索引"
        value={cfgNum(config, "start")}
        onChange={(v) => onChange({ ...config, start: v })}
        placeholder="0"
      />
      <NumberField
        label="结束索引 (可选)"
        value={cfgNum(config, "end")}
        onChange={(v) => onChange({ ...config, end: v })}
        placeholder="留空到末尾"
        hint="支持负数索引，如 -1 表示最后一个元素"
      />
    </>
  );
}
