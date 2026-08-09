import { TextField, cfgStr } from "./ConfigField";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
}

export default function FilterConfig({ config, onChange, upstreamOutputs }: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <TextField
        label="过滤条件"
        value={cfgStr(config, "condition")}
        onChange={(v) => onChange({ ...config, condition: v })}
        placeholder={'{{status}} == "active"'}
        hint="条件为 true 时放行，否则阻断"
        upstream={up}
      />
    </>
  );
}
