import { KeyValueEditor, jsonToKvEntries } from "./StructuredEditors";
import type { KvEntry } from "./StructuredEditors";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function SetFieldsConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <KeyValueEditor
        label="字段赋值"
        value={jsonToKvEntries(config.fields)}
        onChange={(v: KvEntry[]) => onChange({ ...config, fields: v })}
        keyPlaceholder="字段名"
        valuePlaceholder="值 (支持 {{var}})"
        hint="每行设定一个输出字段的值"
      />
    </>
  );
}
