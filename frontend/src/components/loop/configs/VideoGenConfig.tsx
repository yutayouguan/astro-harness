import { TextField, NumberField, SelectField, ToggleField, FilePathField, FileArrayField, cfgStr, cfgNum, cfgBool, cfgStrArray } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
}

const MODE_OPTIONS = [
  { value: "text_to_video", label: "文生视频" },
  { value: "image_to_video", label: "图生视频" },
  { value: "reference", label: "多模态参考生成 (H3)" },
  { value: "video_to_video", label: "视频转视频 (换脸/风格)" },
];

const ASPECT_RATIO_OPTIONS = [
  { value: "16:9", label: "16:9 横版" },
  { value: "9:16", label: "9:16 竖版" },
  { value: "1:1", label: "1:1 方形" },
  { value: "4:3", label: "4:3" },
  { value: "3:4", label: "3:4" },
  { value: "adaptive", label: "自适应 (跟随图片)" },
];

const RESOLUTION_OPTIONS = [
  { value: "768P", label: "768P" },
  { value: "2K", label: "2K (H3)" },
  { value: "720P", label: "720P (旧版)" },
  { value: "1080P", label: "1080P (旧版)" },
];

export default function VideoGenConfig({ config, onChange, upstreamOutputs }: ConfigProps) {
  const mode = cfgStr(config, "mode", "text_to_video");
  const isReference = mode === "reference";
  const isImageToVideo = mode === "image_to_video";
  const isVideoToVideo = mode === "video_to_video";
  const up = upstreamOutputs ?? [];

  return (
    <>
      <SelectField
        label="生成模式"
        value={mode}
        onChange={(v) => onChange({ ...config, mode: v })}
        options={MODE_OPTIONS}
      />
      <TextField
        label="生成提示词"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        placeholder="描述你要生成的视频…"
        multiline
        hint="支持 {{var}} 引用上游变量。可使用 [运镜] 标记控制镜头运动"
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
        mediaType="video"
      />

      {/* 图生视频 / 参考模式：首帧/末帧 */}
      {(isImageToVideo || isReference) && (
        <>
          <FilePathField
            label="首帧图片"
            value={cfgStr(config, "first_frame_image")}
            onChange={(v) => onChange({ ...config, first_frame_image: v })}
            accept="image"
            upstream={up}
            hint="I2V 起始帧，画面比例将自动适配"
          />
          <FilePathField
            label="末帧图片"
            value={cfgStr(config, "last_frame_image")}
            onChange={(v) => onChange({ ...config, last_frame_image: v })}
            accept="image"
            upstream={up}
            hint="可选，控制视频结束画面"
          />
        </>
      )}

      {/* 视频转视频：源视频 */}
      {isVideoToVideo && (
        <FilePathField
          label="源视频"
          value={cfgStr(config, "source_video")}
          onChange={(v) => onChange({ ...config, source_video: v })}
          accept="video"
          upstream={up}
          hint="要替换/转换的原始视频"
        />
      )}

      {/* 参考图片 */}
      <FileArrayField
        label="参考图片"
        value={cfgStrArray(config, "reference_images")}
        onChange={(v) => onChange({ ...config, reference_images: v })}
        accept="image"
        upstream={up}
        max={isReference ? 9 : 3}
        hint="用于角色/风格/场景一致性"
      />

      {/* H3 多模态参考：参考视频和音频 */}
      {isReference && (
        <>
          <FileArrayField
            label="参考视频"
            value={cfgStrArray(config, "reference_videos")}
            onChange={(v) => onChange({ ...config, reference_videos: v })}
            accept="video"
            upstream={up}
            max={3}
            hint="每个 2-15s，总时长 ≤15s"
          />
          <FileArrayField
            label="参考音频"
            value={cfgStrArray(config, "reference_audios")}
            onChange={(v) => onChange({ ...config, reference_audios: v })}
            accept="audio"
            upstream={up}
            max={3}
            hint="每个 2-15s，总时长 ≤15s"
          />
        </>
      )}

      <NumberField
        label="时长 (秒)"
        value={cfgNum(config, "duration_seconds")}
        onChange={(v) => onChange({ ...config, duration_seconds: v })}
        min={4}
        max={15}
        placeholder="6"
      />
      <SelectField
        label="画面比例"
        value={cfgStr(config, "aspect_ratio", "16:9")}
        onChange={(v) => onChange({ ...config, aspect_ratio: v })}
        options={ASPECT_RATIO_OPTIONS}
      />
      <SelectField
        label="分辨率"
        value={cfgStr(config, "resolution", "768P")}
        onChange={(v) => onChange({ ...config, resolution: v })}
        options={RESOLUTION_OPTIONS}
      />
      <ToggleField
        label="提示词增强 (H3 Context-IR)"
        value={cfgBool(config, "enhance_prompt")}
        onChange={(v) => onChange({ ...config, enhance_prompt: v })}
        hint="深度理解多模态上下文，自动优化提示词（H3 专属）"
      />
      <ToggleField
        label="提示词优化"
        value={cfgBool(config, "prompt_optimizer", true)}
        onChange={(v) => onChange({ ...config, prompt_optimizer: v })}
        hint="MiniMax 内置提示词优化，关闭可获得更精确的控制"
      />
    </>
  );
}
