import { TextField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function ScheduledTriggerConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="调度表达式"
        value={cfgStr(config, "schedule")}
        onChange={(v) => onChange({ ...config, schedule: v })}
        placeholder="every:5m / 0 9 * * * / once:RFC3339"
        hint="支持 every:Nm/Nh、五段 cron、once:RFC3339"
      />
      <TextField
        label="时区"
        value={cfgStr(config, "timezone")}
        onChange={(v) => onChange({ ...config, timezone: v })}
        placeholder="Asia/Shanghai (可选)"
      />
    </>
  );
}
