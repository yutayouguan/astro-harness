import { TextField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import { TagInput, jsonToStringArray } from "./StructuredEditors";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function SentimentAnalysisConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="输入文本"
        value={cfgStr(config, "text_template")}
        onChange={(v) => onChange({ ...config, text_template: v })}
        placeholder="输入要分析的文本…"
        multiline
        hint="支持 {{var}} 引用上游变量"
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
      />
      <TagInput
        label="自定义标签"
        value={jsonToStringArray(config.custom_labels)}
        onChange={(v: string[]) => onChange({ ...config, custom_labels: v })}
        placeholder="回车添加标签"
        hint="留空使用默认情感分类（积极/消极/中性）"
      />
    </>
  );
}
