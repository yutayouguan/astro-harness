import { TextField, Section, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function ConditionalConfig({ config, onChange }: ConfigProps) {
  return (
    <Section title="条件分支">
      <TextField
        label="条件列表 (JSON)"
        value={cfgStr(config, "conditions")}
        onChange={(v) => onChange({ ...config, conditions: v })}
        placeholder={'[\n  {"id": "yes", "label": "满足条件", "expression": "{{result}} == true"},\n  {"id": "no", "label": "不满足", "expression": ""}\n]'}
        multiline
        hint="每个条件对应一个输出端口。最后一条无 expression 为 else 分支"
      />
    </Section>
  );
}
