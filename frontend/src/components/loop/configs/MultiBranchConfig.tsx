import { TextField, Section, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function MultiBranchConfig({ config, onChange }: ConfigProps) {
  return (
    <Section title="分支定义">
      <TextField
        label="分支列表 (JSON)"
        value={cfgStr(config, "branches")}
        onChange={(v) => onChange({ ...config, branches: v })}
        placeholder={'[\n  {"id": "a", "label": "分支 A", "condition": "{{type}} == \'a\'"},\n  {"id": "b", "label": "分支 B", "condition": "{{type}} == \'b\'"},\n  {"id": "default", "label": "默认"}\n]'}
        multiline
        hint="condition 为空或省略的分支为默认分支"
      />
    </Section>
  );
}
