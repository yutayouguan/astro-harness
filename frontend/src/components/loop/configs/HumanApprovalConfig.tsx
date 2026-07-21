import { TextField, NumberField, cfgStr, cfgNum } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function HumanApprovalConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="审批提示"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="请确认以下操作是否继续…"
        multiline
        hint="支持 {{var}} 引用上游变量"
      />
      <NumberField
        label="超时时间 (秒)"
        value={cfgNum(config, "timeout_seconds")}
        onChange={(v) => onChange({ ...config, timeout_seconds: v })}
        placeholder="3600"
        hint="超时后自动拒绝。留空则无限等待"
      />
    </>
  );
}
