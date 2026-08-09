import { RowListEditor, jsonToRowList } from "./StructuredEditors";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function OutputConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <RowListEditor
        label="输出字段"
        columns={[
          { key: "name", label: "字段名", placeholder: "如 result" },
          { key: "type", label: "类型", placeholder: "string" },
          { key: "desc", label: "描述", placeholder: "可选" },
        ]}
        value={jsonToRowList(config.output_fields)}
        onChange={(v) => onChange({ ...config, output_fields: v })}
        hint="定义此 Loop 的最终输出字段"
      />
    </>
  );
}
