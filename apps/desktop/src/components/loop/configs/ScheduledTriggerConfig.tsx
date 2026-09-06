import { TextField, SelectField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const TIMEZONE_OPTIONS = [
  { value: "", label: "系统默认" },
  { value: "Asia/Shanghai", label: "Asia/Shanghai (北京)" },
  { value: "Asia/Tokyo", label: "Asia/Tokyo (东京)" },
  { value: "America/New_York", label: "America/New_York (纽约)" },
  { value: "America/Los_Angeles", label: "America/Los_Angeles (洛杉矶)" },
  { value: "Europe/London", label: "Europe/London (伦敦)" },
  { value: "UTC", label: "UTC" },
];

export default function ScheduledTriggerConfig({
  config,
  onChange,
}: ConfigProps) {
  return (
    <>
      <TextField
        label="调度表达式"
        value={cfgStr(config, "schedule")}
        onChange={(v) => onChange({ ...config, schedule: v })}
        placeholder="every:5m / 0 9 * * *"
        hint="支持 every:Nm/Nh/Nd 和五段 cron"
      />
      <SelectField
        label="时区"
        value={cfgStr(config, "timezone")}
        onChange={(v) => onChange({ ...config, timezone: v })}
        options={TIMEZONE_OPTIONS}
        hint="IANA 标准时区"
      />
    </>
  );
}
