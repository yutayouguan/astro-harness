import { TextField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function ParameterExtractionConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="提取指令"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="从以下文本中提取姓名、电话、邮箱"
        multiline
      />
      <TextField
        label="输出 Schema (JSON)"
        value={cfgStr(config, "output_schema")}
        onChange={(v) => onChange({ ...config, output_schema: v })}
        placeholder={'[\n  {"name": "姓名", "field_type": "string"},\n  {"name": "电话", "field_type": "string"}\n]'}
        multiline
        hint="定义需要提取的字段列表"
      />
      <TextField
        label="供应商 (可选)"
        value={cfgStr(config, "provider_id")}
        onChange={(v) => onChange({ ...config, provider_id: v })}
      />
      <TextField
        label="模型 (可选)"
        value={cfgStr(config, "model")}
        onChange={(v) => onChange({ ...config, model: v })}
      />
    </>
  );
}
