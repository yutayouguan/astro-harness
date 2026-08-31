import { TextField, NumberField, cfgStr, cfgNum } from "./ConfigField";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
}

export default function LoopConfig({
  config,
  onChange,
  upstreamOutputs,
}: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <NumberField
        label="最大迭代次数"
        value={cfgNum(config, "max_iterations")}
        onChange={(v) => onChange({ ...config, max_iterations: v })}
        placeholder="10"
        min={1}
        max={1000}
      />
      <TextField
        label="终止条件 (可选)"
        value={cfgStr(config, "break_condition")}
        onChange={(v) => onChange({ ...config, break_condition: v })}
        placeholder="{{count}} >= 5"
        hint="每轮迭代检查，满足则退出循环"
        upstream={up}
      />
      <TextField
        label="遍历数组变量 (可选)"
        value={cfgStr(config, "collection_var")}
        onChange={(v) => onChange({ ...config, collection_var: v })}
        placeholder="{{items}}"
        hint="设置后按数组元素逐一迭代 (for-each)"
        upstream={up}
      />
    </>
  );
}
