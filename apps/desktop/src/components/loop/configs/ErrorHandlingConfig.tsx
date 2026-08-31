/** 错误处理配置 —— 所有节点共用底部区域 */

import { SelectField, NumberField, TextField, cfgStr } from "./ConfigField";

interface Props {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function ErrorHandlingConfig({ config, onChange }: Props) {
  const onError = (config.on_error as string) || "abort";
  const retryCount =
    typeof config.retry_count === "number" ? config.retry_count : 0;
  const retryInterval =
    typeof config.retry_interval_ms === "number" ? config.retry_interval_ms : 0;

  return (
    <div className="loop-config-section">
      <div className="loop-config-section-title">错误处理</div>
      <SelectField
        label="出错时"
        value={onError}
        onChange={(v) => onChange({ ...config, on_error: v })}
        options={[
          { value: "abort", label: "中止整个运行" },
          { value: "skip", label: "跳过该节点并继续" },
          { value: "fallback", label: "使用兜底输出继续" },
        ]}
      />
      {onError === "fallback" && (
        <TextField
          label="兜底输出值"
          value={cfgStr(config, "fallback_value")}
          onChange={(v) => onChange({ ...config, fallback_value: v })}
          placeholder='如 {"result": "默认值"}'
          hint="节点出错时使用此值作为输出，传递给下游"
        />
      )}
      {onError !== "abort" && (
        <div className="loop-config-row">
          <NumberField
            label="重试次数"
            value={retryCount}
            onChange={(v) => onChange({ ...config, retry_count: v ?? 0 })}
            min={0}
            max={10}
            hint="0 = 不重试"
          />
          <NumberField
            label="重试间隔(ms)"
            value={retryInterval}
            onChange={(v) => onChange({ ...config, retry_interval_ms: v ?? 0 })}
            min={0}
            max={60000}
            hint="0 = 立即重试"
          />
        </div>
      )}
    </div>
  );
}
