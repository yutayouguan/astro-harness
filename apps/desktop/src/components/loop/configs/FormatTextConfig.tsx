import { AiAssistField, cfgStr } from "./ConfigField";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
  aiProviderId?: string;
  aiModel?: string;
}

export default function FormatTextConfig({
  config,
  onChange,
  upstreamOutputs,
  aiProviderId,
  aiModel,
}: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <AiAssistField
        label="文本模板"
        value={cfgStr(config, "template")}
        onChange={(v) => onChange({ ...config, template: v })}
        multiline
        placeholder={"你好 {{name}}，你的订单 {{order_id}} 已确认。"}
        hint="用 {{变量名}} 引用上游输出"
        task="格式化文本模板"
        aiProviderId={aiProviderId}
        aiModel={aiModel}
        upstream={up}
      />
    </>
  );
}
