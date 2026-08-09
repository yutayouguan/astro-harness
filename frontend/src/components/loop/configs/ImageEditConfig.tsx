import { TextField, SelectField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const OPERATION_OPTIONS = [
  { value: "inpaint", label: "局部重绘 (Inpaint)" },
  { value: "outpaint", label: "扩图 (Outpaint)" },
  { value: "remove_bg", label: "移除背景" },
  { value: "upscale", label: "超分辨率" },
  { value: "style_transfer", label: "风格迁移" },
  { value: "variation", label: "生成变体" },
];

export default function ImageEditConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <SelectField
        label="操作类型"
        value={cfgStr(config, "operation", "inpaint")}
        onChange={(v) => onChange({ ...config, operation: v })}
        options={OPERATION_OPTIONS}
      />
      <TextField
        label="输入图片"
        value={cfgStr(config, "input_image")}
        onChange={(v) => onChange({ ...config, input_image: v })}
        placeholder="图片路径或 {{var}}"
      />
      <TextField
        label="蒙版图片"
        value={cfgStr(config, "mask_image")}
        onChange={(v) => onChange({ ...config, mask_image: v })}
        placeholder="用于 inpaint/outpaint 的蒙版（可选）"
      />
      <TextField
        label="编辑提示词"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="描述要编辑的效果…"
        multiline
        hint="支持 {{var}} 引用上游变量"
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
        mediaType="image"
      />
    </>
  );
}
