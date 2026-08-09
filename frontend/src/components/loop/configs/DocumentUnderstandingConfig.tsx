import { TextField, SelectField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const TASK_OPTIONS = [
  { value: "ocr", label: "OCR 文字提取" },
  { value: "table_extract", label: "表格提取" },
  { value: "form_extract", label: "表单字段提取" },
  { value: "layout_analysis", label: "版面分析" },
  { value: "qa", label: "文档问答" },
];

const INPUT_TYPE_OPTIONS = [
  { value: "image", label: "图片" },
  { value: "pdf", label: "PDF" },
  { value: "url", label: "网页 URL" },
];

export default function DocumentUnderstandingConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <SelectField
        label="任务类型"
        value={cfgStr(config, "task", "ocr")}
        onChange={(v) => onChange({ ...config, task: v })}
        options={TASK_OPTIONS}
      />
      <SelectField
        label="输入类型"
        value={cfgStr(config, "input_type", "image")}
        onChange={(v) => onChange({ ...config, input_type: v })}
        options={INPUT_TYPE_OPTIONS}
      />
      <TextField
        label="输入路径"
        value={cfgStr(config, "input_path")}
        onChange={(v) => onChange({ ...config, input_path: v })}
        placeholder="文件路径、URL 或 {{var}}"
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
      />
      <TextField
        label="提取指令"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        multiline
        placeholder="描述要提取的内容或问题…"
        hint="用于 QA 或自定义提取"
      />
    </>
  );
}
