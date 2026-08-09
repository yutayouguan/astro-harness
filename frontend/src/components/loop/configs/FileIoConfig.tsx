import { TextField, SelectField, FilePathField, cfgStr } from "./ConfigField";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
}

const OPERATION_OPTIONS = [
  { value: "read", label: "读取文件" },
  { value: "write", label: "写入文件" },
  { value: "append", label: "追加写入" },
  { value: "copy", label: "复制文件" },
  { value: "move", label: "移动文件" },
  { value: "delete", label: "删除文件" },
  { value: "list", label: "列出目录" },
];

export default function FileIoConfig({ config, onChange, upstreamOutputs }: ConfigProps) {
  const up = upstreamOutputs ?? [];
  const operation = cfgStr(config, "operation", "read");
  return (
    <>
      <SelectField
        label="操作类型"
        value={operation}
        onChange={(v) => onChange({ ...config, operation: v })}
        options={OPERATION_OPTIONS}
      />
      <FilePathField
        label="路径"
        value={cfgStr(config, "path")}
        onChange={(v) => onChange({ ...config, path: v })}
        placeholder="文件或目录路径"
        hint="支持 {{var}} 引用"
        upstream={up}
      />
      {(operation === "copy" || operation === "move") && (
        <FilePathField
          label="目标路径"
          value={cfgStr(config, "dest_path")}
          onChange={(v) => onChange({ ...config, dest_path: v })}
          placeholder="用于复制/移动的目标路径"
          upstream={up}
        />
      )}
      {(operation === "write" || operation === "append") && (
        <TextField
          label="写入内容"
          value={cfgStr(config, "content_template")}
          onChange={(v) => onChange({ ...config, content_template: v })}
          placeholder="写入/追加的内容"
          multiline
          hint="支持 {{var}} 引用上游变量"
        />
      )}
    </>
  );
}
