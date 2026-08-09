import { TextField, SelectField, NumberField, cfgStr, cfgNum } from "./ConfigField";
import { KeyValueEditor, KvEntry, jsonToKvEntries, kvEntriesToObj } from "./StructuredEditors";

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

const AUTH_OPTIONS = [
  { value: "none", label: "无" },
  { value: "bearer", label: "Bearer Token" },
  { value: "basic", label: "Basic Auth" },
  { value: "api_key", label: "API Key (Header)" },
];

function authPlaceholder(type: string): string {
  switch (type) {
    case "bearer": return "Token";
    case "basic": return "user:password";
    case "api_key": return "Key 值";
    default: return "";
  }
}

export default function HttpRequestConfig({ config, onChange }: ConfigProps) {
  const authType = cfgStr(config, "auth_type", "none");

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
      <SelectField
        label="认证方式"
        value={authType}
        onChange={(v) => onChange({ ...config, auth_type: v })}
        options={AUTH_OPTIONS}
      />
      {authType !== "none" && (
        <TextField
          label="认证凭据"
          value={cfgStr(config, "auth_credential")}
          onChange={(v) => onChange({ ...config, auth_credential: v })}
          placeholder={authPlaceholder(authType)}
        />
      )}
      <KeyValueEditor
        label="请求头"
        value={jsonToKvEntries(config.headers)}
        onChange={(v: KvEntry[]) => onChange({ ...config, headers: kvEntriesToObj(v) })}
        keyPlaceholder="Header 名"
        valuePlaceholder="值"
        hint="常用头如 Content-Type, Accept"
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
