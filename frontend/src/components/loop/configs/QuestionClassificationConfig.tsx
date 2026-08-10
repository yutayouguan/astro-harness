import { cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import { RowListEditor, jsonToRowList } from "./StructuredEditors";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function QuestionClassificationConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <RowListEditor
        label="分类列表"
        columns={[
          { key: "id", label: "ID", placeholder: "billing", flex: 1 },
          { key: "label", label: "标签", placeholder: "计费问题", flex: 1.5 },
          { key: "description", label: "描述", placeholder: "可选", flex: 2 },
        ]}
        value={jsonToRowList(config.classes)}
        onChange={(v) => onChange({ ...config, classes: v })}
        hint="每个分类对应一个输出端口"
        addLabel="添加分类"
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
