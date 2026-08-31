import {
  AiAssistField,
  NumberField,
  SelectField,
  FilePathField,
  FileArrayField,
  cfgStr,
  cfgNum,
  cfgStrArray,
} from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
  aiProviderId?: string;
  aiModel?: string;
}

const OPERATION_OPTIONS = [
  { value: "inpaint", label: "局部重绘 (Inpaint)" },
  { value: "outpaint", label: "扩图 (Outpaint)" },
  { value: "remove_bg", label: "移除背景" },
  { value: "upscale", label: "超分辨率" },
  { value: "style_transfer", label: "风格迁移" },
  { value: "face_swap", label: "换脸" },
  { value: "variation", label: "生成变体" },
  { value: "colorize", label: "上色" },
  { value: "restore", label: "修复/增强" },
];

export default function ImageEditConfig({
  config,
  onChange,
  upstreamOutputs,
  aiProviderId,
  aiModel,
}: ConfigProps) {
  const op = cfgStr(config, "operation", "inpaint");
  const up = upstreamOutputs ?? [];
  return (
    <>
      <SelectField
        label="操作类型"
        value={op}
        onChange={(v) => onChange({ ...config, operation: v })}
        options={OPERATION_OPTIONS}
      />
      <FilePathField
        label="输入图片"
        value={cfgStr(config, "input_image")}
        onChange={(v) => onChange({ ...config, input_image: v })}
        accept="image"
        upstream={up}
      />
      {(op === "inpaint" || op === "outpaint") && (
        <FilePathField
          label="蒙版图片"
          value={cfgStr(config, "mask_image")}
          onChange={(v) => onChange({ ...config, mask_image: v })}
          accept="image"
          upstream={up}
          hint="可选"
        />
      )}
      <AiAssistField
        label="编辑提示词"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="描述要编辑的效果…"
        hint="支持 {{var}} 引用上游变量"
        task="图片编辑提示词"
        aiProviderId={aiProviderId}
        aiModel={aiModel}
        upstream={up}
      />
      {(op === "style_transfer" ||
        op === "face_swap" ||
        op === "variation") && (
        <FileArrayField
          label="参考图片"
          value={cfgStrArray(config, "reference_images")}
          onChange={(v) => onChange({ ...config, reference_images: v })}
          accept="image"
          upstream={up}
          hint="风格参考或人脸参考图"
        />
      )}
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
        mediaType="image"
      />
      {(op === "style_transfer" || op === "face_swap" || op === "inpaint") && (
        <NumberField
          label="强度"
          value={cfgNum(config, "strength")}
          onChange={(v) => onChange({ ...config, strength: v })}
          min={0}
          max={1}
          step={0.05}
          placeholder="0.8"
        />
      )}
      {op === "upscale" && (
        <SelectField
          label="放大倍数"
          value={cfgStr(config, "scale", "2")}
          onChange={(v) => onChange({ ...config, scale: v })}
          options={[
            { value: "2", label: "2×" },
            { value: "4", label: "4×" },
          ]}
        />
      )}
    </>
  );
}
