import { TextField, SelectField, FilePathField, cfgStr } from "./ConfigField";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
}

const EVENT_OPTIONS = [
  { value: "created", label: "文件创建" },
  { value: "modified", label: "文件修改" },
  { value: "deleted", label: "文件删除" },
  { value: "any", label: "任何变化" },
];

export default function FileWatchConfig({ config, onChange, upstreamOutputs }: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <FilePathField
        label="监控目录"
        value={cfgStr(config, "watch_path")}
        onChange={(v) => onChange({ ...config, watch_path: v })}
        placeholder="/path/to/watch"
        upstream={up}
      />
      <SelectField
        label="触发事件"
        value={cfgStr(config, "event_type", "any")}
        onChange={(v) => onChange({ ...config, event_type: v })}
        options={EVENT_OPTIONS}
      />
      <TextField
        label="文件名匹配"
        value={cfgStr(config, "pattern")}
        onChange={(v) => onChange({ ...config, pattern: v })}
        placeholder="*.pdf, *.jpg"
        hint="glob 模式，留空匹配所有文件"
      />
    </>
  );
}
