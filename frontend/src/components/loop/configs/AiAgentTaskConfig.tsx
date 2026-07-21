import { TextField, NumberField, cfgStr, cfgNum } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function AiAgentTaskConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="Agent ID"
        value={cfgStr(config, "agent_id")}
        onChange={(v) => onChange({ ...config, agent_id: v })}
        placeholder="default"
      />
      <TextField
        label="指令"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="描述你希望智能体执行的操作。用 {{text}} 引用上一步的输出。"
        multiline
        hint="不写 {{}} 时，上游的输出会自动接到指令末尾"
      />
      <TextField
        label="供应商 (可选)"
        value={cfgStr(config, "provider_id")}
        onChange={(v) => onChange({ ...config, provider_id: v })}
        placeholder="留空使用默认"
      />
      <TextField
        label="模型 (可选)"
        value={cfgStr(config, "model")}
        onChange={(v) => onChange({ ...config, model: v })}
        placeholder="留空使用默认"
      />
      <NumberField
        label="最大工具轮次"
        value={cfgNum(config, "max_tool_rounds")}
        onChange={(v) => onChange({ ...config, max_tool_rounds: v })}
        placeholder="90"
        min={1}
        max={200}
      />
    </>
  );
}
