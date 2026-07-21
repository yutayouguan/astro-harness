import { SelectField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function MergeConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <SelectField
        label="合并模式"
        value={cfgStr(config, "mode", "wait_all")}
        onChange={(v) => onChange({ ...config, mode: v })}
        options={[
          { value: "wait_all", label: "等待全部 (Wait All)" },
          { value: "wait_any", label: "任一到达 (Wait Any)" },
        ]}
      />
    </>
  );
}
