import { TextField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";

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
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
      />
    </>
  );
}
