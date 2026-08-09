import { TextField, SelectField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const CHANNEL_OPTIONS = [
  { value: "system", label: "系统通知" },
  { value: "email", label: "邮件" },
  { value: "webhook", label: "Webhook" },
];

export default function SendNotificationConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <SelectField
        label="通知渠道"
        value={cfgStr(config, "channel", "system")}
        onChange={(v) => onChange({ ...config, channel: v })}
        options={CHANNEL_OPTIONS}
      />
      <TextField
        label="标题"
        value={cfgStr(config, "title_template")}
        onChange={(v) => onChange({ ...config, title_template: v })}
        placeholder="通知标题"
        hint="支持 {{var}} 引用"
      />
      <TextField
        label="内容"
        value={cfgStr(config, "body_template")}
        onChange={(v) => onChange({ ...config, body_template: v })}
        placeholder="通知正文…"
        multiline
        hint="支持 {{var}} 引用上游变量"
      />
      <TextField
        label="接收地址"
        value={cfgStr(config, "recipient")}
        onChange={(v) => onChange({ ...config, recipient: v })}
        placeholder="邮箱、Webhook URL 等"
        hint="系统通知可留空"
      />
    </>
  );
}
