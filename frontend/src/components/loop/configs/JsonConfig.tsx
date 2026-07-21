import { TextField, SelectField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const MODE_OPTIONS = [
  { value: "parse", label: "Parse (字符串→对象)" },
  { value: "stringify", label: "Stringify (对象→字符串)" },
  { value: "transform", label: "Transform (JQ 表达式)" },
];

export default function JsonConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <SelectField
        label="操作模式"
        value={cfgStr(config, "mode", "parse")}
        onChange={(v) => onChange({ ...config, mode: v })}
        options={MODE_OPTIONS}
      />
      <TextField
        label="表达式"
        value={cfgStr(config, "expression")}
        onChange={(v) => onChange({ ...config, expression: v })}
        multiline
        placeholder={".data[] | {name, age}"}
        hint="Transform 模式下填写 JQ/JSONPath 表达式"
      />
    </>
  );
}
