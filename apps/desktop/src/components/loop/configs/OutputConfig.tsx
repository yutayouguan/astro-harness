import { TextField, SelectField, cfgStr } from "./ConfigField";
import { RowListEditor, jsonToRowList } from "./StructuredEditors";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const EXPORT_MODE_OPTIONS = [
  { value: "none", label: "仅返回数据" },
  { value: "json", label: "导出为 JSON 文件" },
  { value: "folder", label: "导出到文件夹（含媒体文件）" },
];

export default function OutputConfig({ config, onChange }: ConfigProps) {
  const exportMode = cfgStr(config, "export_mode", "none");
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
      <SelectField
        label="导出方式"
        value={exportMode}
        onChange={(v) => onChange({ ...config, export_mode: v })}
        options={EXPORT_MODE_OPTIONS}
      />
      {exportMode !== "none" && (
        <TextField
          label="导出路径"
          value={cfgStr(config, "export_path")}
          onChange={(v) => onChange({ ...config, export_path: v })}
          placeholder={exportMode === "folder" ? "~/Desktop/workflow-output" : "~/Desktop/output.json"}
          hint="支持 ~ 和 {{var}} 引用"
        />
      )}
    </>
  );
}
