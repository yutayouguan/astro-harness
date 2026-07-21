import { TextField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function RunLoopConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="目标 Loop ID"
        value={cfgStr(config, "workflow_id")}
        onChange={(v) => onChange({ ...config, workflow_id: v })}
        placeholder="要调用的子工作流 ID"
      />
      <TextField
        label="输入映射 (JSON)"
        value={cfgStr(config, "input_mapping")}
        onChange={(v) => onChange({ ...config, input_mapping: v })}
        multiline
        placeholder={'[\n  {"field": "query", "value": "{{text}}"}\n]'}
        hint="将当前上下文的变量映射到子 Loop 的输入"
      />
    </>
  );
}
