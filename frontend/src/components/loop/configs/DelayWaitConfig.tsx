import { NumberField, cfgNum } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function DelayWaitConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <NumberField
        label="等待时间 (秒)"
        value={cfgNum(config, "seconds")}
        onChange={(v) => onChange({ ...config, seconds: v })}
        min={1}
        max={86400}
        placeholder="5"
        hint="1 - 86400"
      />
    </>
  );
}
