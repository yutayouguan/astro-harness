import { TextField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function FormatTextConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="文本模板"
        value={cfgStr(config, "template")}
        onChange={(v) => onChange({ ...config, template: v })}
        multiline
        placeholder={"你好 {{name}}，你的订单 {{order_id}} 已确认。"}
        hint="用 {{变量名}} 引用上游输出"
      />
    </>
  );
}
