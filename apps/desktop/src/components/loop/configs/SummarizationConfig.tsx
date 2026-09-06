import {
  TextField,
  NumberField,
  SelectField,
  cfgStr,
  cfgNum,
} from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
}

const STYLE_OPTIONS = [
  { value: "concise", label: "简洁摘要" },
  { value: "detailed", label: "详细摘要" },
  { value: "bullet", label: "要点列表" },
  { value: "headline", label: "一句话标题" },
];

export default function SummarizationConfig({
  config,
  onChange,
  upstreamOutputs,
}: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <TextField
        label="输入文本"
        value={cfgStr(config, "text_template")}
        onChange={(v) => onChange({ ...config, text_template: v })}
        placeholder="输入要摘要的文本…"
        multiline
        hint="支持 {{var}} 引用上游变量"
        upstream={up}
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
      />
      <SelectField
        label="摘要风格"
        value={cfgStr(config, "style", "concise")}
        onChange={(v) => onChange({ ...config, style: v })}
        options={STYLE_OPTIONS}
      />
      <NumberField
        label="最大长度 (字)"
        value={cfgNum(config, "max_length")}
        onChange={(v) => onChange({ ...config, max_length: v })}
        min={10}
        max={5000}
        placeholder="200"
      />
    </>
  );
}
