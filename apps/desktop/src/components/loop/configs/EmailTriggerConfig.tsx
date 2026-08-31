import { TextField, SelectField, PasswordField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const PROTOCOL_OPTIONS = [
  { value: "imap", label: "IMAP" },
  { value: "pop3", label: "POP3" },
];

export default function EmailTriggerConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <SelectField
        label="协议"
        value={cfgStr(config, "protocol", "imap")}
        onChange={(v) => onChange({ ...config, protocol: v })}
        options={PROTOCOL_OPTIONS}
      />
      <TextField
        label="服务器地址"
        value={cfgStr(config, "host")}
        onChange={(v) => onChange({ ...config, host: v })}
        placeholder="imap.example.com"
        hint="如 imap.gmail.com"
      />
      <TextField
        label="用户名"
        value={cfgStr(config, "username")}
        onChange={(v) => onChange({ ...config, username: v })}
        placeholder="your@email.com"
        hint="完整邮箱地址"
      />
      <PasswordField
        label="密码 / 授权码"
        value={cfgStr(config, "password")}
        onChange={(v) => onChange({ ...config, password: v })}
        placeholder="••••••••"
      />
      <TextField
        label="过滤条件"
        value={cfgStr(config, "filter")}
        onChange={(v) => onChange({ ...config, filter: v })}
        placeholder="发件人、主题关键词等"
        hint="留空接收所有新邮件"
      />
    </>
  );
}
