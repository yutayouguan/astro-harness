import { TextField, SelectField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const INPUT_TYPE_OPTIONS = [
  { value: "file", label: "本地文件" },
  { value: "url", label: "图片 URL" },
];

export default function VisionUnderstandingConfig({
  config,
  onChange,
}: ConfigProps) {
  return (
    <>
      <SelectField
        label="输入类型"
        value={cfgStr(config, "input_type", "file")}
        onChange={(v) => onChange({ ...config, input_type: v })}
        options={INPUT_TYPE_OPTIONS}
      />
      <TextField
        label="图片路径 / URL"
        value={cfgStr(config, "input_path")}
        onChange={(v) => onChange({ ...config, input_path: v })}
        placeholder={
          cfgStr(config, "input_type", "file") === "url"
            ? "https://..."
            : "图片文件路径或 {{var}}"
        }
        hint="支持 jpg/png/webp/gif"
      />
      <TextField
        label="提示词"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="描述这张图片 / 提取图中的文字 / 分析图片内容…"
        multiline
        hint="告诉模型你想了解什么，支持 {{var}} 引用"
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
