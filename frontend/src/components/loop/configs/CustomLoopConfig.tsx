import { TextField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function CustomLoopConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="引用的 Loop ID"
        value={cfgStr(config, "workflow_id")}
        onChange={(v) => onChange({ ...config, workflow_id: v })}
        placeholder="被引用的工作流 ID"
        hint="此节点运行时会调用该 Loop 作为子工作流执行"
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
