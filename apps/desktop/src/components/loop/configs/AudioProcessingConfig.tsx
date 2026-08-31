import {
  SelectField,
  NumberField,
  FileArrayField,
  cfgStr,
  cfgNum,
  cfgStrArray,
} from "./ConfigField";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
  aiProviderId?: string;
  aiModel?: string;
}

const OPERATION_OPTIONS = [
  { value: "concat", label: "拼接 (Concat)" },
  { value: "trim", label: "裁切 (Trim)" },
  { value: "mix", label: "混音 (Mix)" },
  { value: "convert", label: "格式转换 (Convert)" },
  { value: "normalize", label: "音量归一化 (Normalize)" },
];

const FORMAT_OPTIONS = [
  { value: "mp3", label: "MP3" },
  { value: "wav", label: "WAV" },
  { value: "ogg", label: "OGG" },
  { value: "flac", label: "FLAC" },
];

export default function AudioProcessingConfig({
  config,
  onChange,
  upstreamOutputs,
}: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <SelectField
        label="操作类型"
        value={cfgStr(config, "operation", "concat")}
        onChange={(v) => onChange({ ...config, operation: v })}
        options={OPERATION_OPTIONS}
      />
      <FileArrayField
        label="输入音频"
        value={cfgStrArray(config, "inputs")}
        onChange={(v) => onChange({ ...config, inputs: v })}
        accept="audio"
        upstream={up}
        hint="拖拽或添加多个音频文件"
      />
      <NumberField
        label="裁切起始 (秒)"
        value={cfgNum(config, "trim_start")}
        onChange={(v) => onChange({ ...config, trim_start: v })}
        placeholder="0"
      />
      <NumberField
        label="裁切结束 (秒)"
        value={cfgNum(config, "trim_end")}
        onChange={(v) => onChange({ ...config, trim_end: v })}
        placeholder="留空到末尾"
      />
      <SelectField
        label="输出格式"
        value={cfgStr(config, "output_format", "mp3")}
        onChange={(v) => onChange({ ...config, output_format: v })}
        options={FORMAT_OPTIONS}
      />
      <NumberField
        label="音量倍率"
        value={cfgNum(config, "volume")}
        onChange={(v) => onChange({ ...config, volume: v })}
        min={0.1}
        max={5.0}
        step={0.1}
        placeholder="1.0"
      />
    </>
  );
}
