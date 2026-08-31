import { SelectField, AiAssistField, cfgStr } from "./ConfigField";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
  aiProviderId?: string;
  aiModel?: string;
}

const LANGUAGE_OPTIONS = [
  { value: "javascript", label: "JavaScript" },
  { value: "python", label: "Python" },
];

export default function CodeConfig({
  config,
  onChange,
  upstreamOutputs,
  aiProviderId,
  aiModel,
}: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <SelectField
        label="语言"
        value={cfgStr(config, "language", "javascript")}
        onChange={(v) => onChange({ ...config, language: v })}
        options={LANGUAGE_OPTIONS}
      />
      <AiAssistField
        label="代码"
        value={cfgStr(config, "source")}
        onChange={(v) => onChange({ ...config, source: v })}
        multiline
        placeholder={
          "// 接收 input 对象，返回 output\nconst output = { result: input.value * 2 };\nreturn output;"
        }
        hint="入参为 input 对象，需返回 output 对象"
        task="代码编写"
        aiProviderId={aiProviderId}
        aiModel={aiModel}
        upstream={up}
      />
    </>
  );
}
