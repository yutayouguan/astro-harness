import { AiAssistField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import { RowListEditor, jsonToRowList } from "./StructuredEditors";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  aiProviderId?: string;
  aiModel?: string;
}

export default function ParameterExtractionConfig({ config, onChange, aiProviderId, aiModel }: ConfigProps) {
  return (
    <>
      <AiAssistField
        label="提取指令"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="从以下文本中提取姓名、电话、邮箱"
        multiline
        task="参数提取指令"
        hint="描述需要从文本中提取的参数"
        aiProviderId={aiProviderId}
        aiModel={aiModel}
      />
      <RowListEditor
        label="输出 Schema"
        columns={[
          { key: "name", label: "字段名", placeholder: "如 phone" },
          { key: "type", label: "类型", placeholder: "string" },
          { key: "desc", label: "描述", placeholder: "手机号码" },
        ]}
        value={jsonToRowList(config.output_schema)}
        onChange={(v) => onChange({ ...config, output_schema: v })}
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
