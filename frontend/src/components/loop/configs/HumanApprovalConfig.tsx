import { NumberField, AiAssistField, cfgStr, cfgNum } from "./ConfigField";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
  aiProviderId?: string;
  aiModel?: string;
}

export default function HumanApprovalConfig({ config, onChange, upstreamOutputs, aiProviderId, aiModel }: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <AiAssistField
        label="审批提示"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="请确认以下操作是否继续…"
        multiline
        hint="支持 {{var}} 引用上游变量"
        task="审批提示信息"
        aiProviderId={aiProviderId}
        aiModel={aiModel}
        upstream={up}
      />
      <NumberField
        label="超时时间 (秒)"
        value={cfgNum(config, "timeout_seconds")}
        onChange={(v) => onChange({ ...config, timeout_seconds: v })}
        placeholder="3600"
        hint="超时后自动拒绝"
      />
    </>
  );
}
