import { TextField, SelectField, NumberField, cfgStr, cfgNum } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const METHOD_OPTIONS = [
  { value: "GET", label: "GET" },
  { value: "POST", label: "POST" },
  { value: "PUT", label: "PUT" },
  { value: "PATCH", label: "PATCH" },
  { value: "DELETE", label: "DELETE" },
];

export default function HttpRequestConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <SelectField
        label="方法"
        value={cfgStr(config, "method", "GET")}
        onChange={(v) => onChange({ ...config, method: v })}
        options={METHOD_OPTIONS}
      />
      <TextField
        label="URL"
        value={cfgStr(config, "url_template")}
        onChange={(v) => onChange({ ...config, url_template: v })}
        placeholder={"https://api.example.com/{{path}}"}
        hint="支持 {{var}} 插值"
      />
      <TextField
        label="请求头 (JSON)"
        value={cfgStr(config, "headers")}
        onChange={(v) => onChange({ ...config, headers: v })}
        multiline
        placeholder={'[["Authorization", "Bearer {{token}}"], ["Content-Type", "application/json"]]'}
      />
      <TextField
        label="请求体 (可选)"
        value={cfgStr(config, "body_template")}
        onChange={(v) => onChange({ ...config, body_template: v })}
        multiline
        placeholder={'{"key": "{{value}}"}'}
      />
      <NumberField
        label="超时 (秒)"
        value={cfgNum(config, "timeout_seconds")}
        onChange={(v) => onChange({ ...config, timeout_seconds: v })}
        placeholder="30"
      />
    </>
  );
}
