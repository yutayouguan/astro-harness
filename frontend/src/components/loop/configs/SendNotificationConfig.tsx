import { TextField, SelectField, AiAssistField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  aiProviderId?: string;
  aiModel?: string;
}

const CHANNEL_OPTIONS = [
  { value: "system", label: "系统通知" },
  { value: "email", label: "邮件" },
  { value: "webhook", label: "Webhook" },
];

export default function SendNotificationConfig({ config, onChange, aiProviderId, aiModel }: ConfigProps) {
  const channel = cfgStr(config, "channel", "system");
  return (
    <>
      <SelectField
        label="通知渠道"
        value={channel}
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
      <AiAssistField
        label="内容"
        value={cfgStr(config, "body_template")}
        onChange={(v) => onChange({ ...config, body_template: v })}
        placeholder="通知正文…"
        multiline
        hint="支持 {{var}} 引用上游变量"
        task="通知内容"
        aiProviderId={aiProviderId}
        aiModel={aiModel}
      />
      {channel !== "system" && (
        <TextField
          label="接收地址"
          value={cfgStr(config, "recipient")}
          onChange={(v) => onChange({ ...config, recipient: v })}
          placeholder="邮箱、Webhook URL 等"
        />
      )}
    </>
  );
}
