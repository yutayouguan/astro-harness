import { TextField, SelectField, cfgStr } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const LANGUAGE_OPTIONS = [
  { value: "javascript", label: "JavaScript" },
  { value: "python", label: "Python" },
];

export default function CodeConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <SelectField
        label="语言"
        value={cfgStr(config, "language", "javascript")}
        onChange={(v) => onChange({ ...config, language: v })}
        options={LANGUAGE_OPTIONS}
      />
      <TextField
        label="代码"
        value={cfgStr(config, "source")}
        onChange={(v) => onChange({ ...config, source: v })}
        multiline
        placeholder={"// 接收 input 对象，返回 output\nconst output = { result: input.value * 2 };\nreturn output;"}
        hint="入参为 input 对象，需返回 output 对象"
      />
    </>
  );
}
