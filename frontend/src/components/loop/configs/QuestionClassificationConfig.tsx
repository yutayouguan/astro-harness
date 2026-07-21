import { TextField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function QuestionClassificationConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="分类列表 (JSON)"
        value={cfgStr(config, "classes")}
        onChange={(v) => onChange({ ...config, classes: v })}
        placeholder={'[\n  {"id": "tech", "label": "技术问题", "description": "编程、部署相关"},\n  {"id": "biz", "label": "业务问题", "description": "产品、运营相关"}\n]'}
        multiline
        hint="每个分类对应一个输出端口"
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
