import { TextField, SelectField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const METHOD_OPTIONS = [
  { value: "GET", label: "GET" },
  { value: "POST", label: "POST" },
  { value: "PUT", label: "PUT" },
];

export default function WebhookTriggerConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="路径"
        value={cfgStr(config, "path")}
        onChange={(v) => onChange({ ...config, path: v })}
        placeholder="/my-webhook"
      />
      <SelectField
        label="方法"
        value={cfgStr(config, "method", "POST")}
        onChange={(v) => onChange({ ...config, method: v })}
        options={METHOD_OPTIONS}
      />
      <TextField
        label="密钥 (可选)"
        value={cfgStr(config, "secret")}
        onChange={(v) => onChange({ ...config, secret: v })}
        placeholder="Bearer token"
      />
    </>
  );
}
