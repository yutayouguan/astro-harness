/** 右侧节点配置面板 — 根据 nodeType 动态渲染对应配置表单 */

import { lazy, Suspense } from "react";
import { Settings2, X } from "lucide-react";
import type { NodeType } from "./loopTypes";

// ── Lazy imports for all 29 config forms ──

const ManualTriggerConfig = lazy(() => import("./configs/ManualTriggerConfig"));
const ScheduledTriggerConfig = lazy(() => import("./configs/ScheduledTriggerConfig"));
const WebhookTriggerConfig = lazy(() => import("./configs/WebhookTriggerConfig"));
const AiAgentTaskConfig = lazy(() => import("./configs/AiAgentTaskConfig"));
const ParameterExtractionConfig = lazy(() => import("./configs/ParameterExtractionConfig"));
const QuestionClassificationConfig = lazy(() => import("./configs/QuestionClassificationConfig"));
const ImageGenConfig = lazy(() => import("./configs/ImageGenConfig"));
const VideoGenConfig = lazy(() => import("./configs/VideoGenConfig"));
const MusicGenConfig = lazy(() => import("./configs/MusicGenConfig"));
const TtsConfig = lazy(() => import("./configs/TtsConfig"));
const SubtitleGenConfig = lazy(() => import("./configs/SubtitleGenConfig"));
const ConditionalConfig = lazy(() => import("./configs/ConditionalConfig"));
const MultiBranchConfig = lazy(() => import("./configs/MultiBranchConfig"));
const FilterConfig = lazy(() => import("./configs/FilterConfig"));
const MergeConfig = lazy(() => import("./configs/MergeConfig"));
const LoopNodeConfig = lazy(() => import("./configs/LoopConfig"));
const HumanApprovalConfig = lazy(() => import("./configs/HumanApprovalConfig"));
const SetFieldsConfig = lazy(() => import("./configs/SetFieldsConfig"));
const FormatTextConfig = lazy(() => import("./configs/FormatTextConfig"));
const JsonConfig = lazy(() => import("./configs/JsonConfig"));
const CodeConfig = lazy(() => import("./configs/CodeConfig"));
const SortConfig = lazy(() => import("./configs/SortConfig"));
const SliceConfig = lazy(() => import("./configs/SliceConfig"));
const AggregateConfig = lazy(() => import("./configs/AggregateConfig"));
const HttpRequestConfig = lazy(() => import("./configs/HttpRequestConfig"));
const RunLoopConfig = lazy(() => import("./configs/RunLoopConfig"));
const DelayWaitConfig = lazy(() => import("./configs/DelayWaitConfig"));
const OutputConfig = lazy(() => import("./configs/OutputConfig"));
const AudioProcessingConfig = lazy(() => import("./configs/AudioProcessingConfig"));

// ── Config form registry ──

const CONFIG_MAP: Record<NodeType, React.LazyExoticComponent<React.ComponentType<ConfigProps>>> = {
  manual_trigger: ManualTriggerConfig,
  scheduled_trigger: ScheduledTriggerConfig,
  webhook_trigger: WebhookTriggerConfig,
  ai_agent_task: AiAgentTaskConfig,
  parameter_extraction: ParameterExtractionConfig,
  question_classification: QuestionClassificationConfig,
  image_generation: ImageGenConfig,
  video_generation: VideoGenConfig,
  music_generation: MusicGenConfig,
  text_to_speech: TtsConfig,
  subtitle_generation: SubtitleGenConfig,
  conditional: ConditionalConfig,
  multi_branch: MultiBranchConfig,
  filter: FilterConfig,
  merge: MergeConfig,
  loop: LoopNodeConfig,
  human_approval: HumanApprovalConfig,
  set_fields: SetFieldsConfig,
  format_text: FormatTextConfig,
  json: JsonConfig,
  code: CodeConfig,
  sort: SortConfig,
  slice: SliceConfig,
  aggregate: AggregateConfig,
  http_request: HttpRequestConfig,
  run_loop: RunLoopConfig,
  delay_wait: DelayWaitConfig,
  output: OutputConfig,
  audio_processing: AudioProcessingConfig,
};

export interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

interface Props {
  nodeType: NodeType;
  label: string;
  config: Record<string, unknown>;
  onLabelChange: (label: string) => void;
  onConfigChange: (config: Record<string, unknown>) => void;
  onClose: () => void;
}

export default function LoopConfigPanel({
  nodeType,
  label,
  config,
  onLabelChange,
  onConfigChange,
  onClose,
}: Props) {
  const ConfigForm = CONFIG_MAP[nodeType];

  return (
    <div className="loop-config-panel">
      <div className="loop-config-panel-header">
        <Settings2 size={16} />
        <span>{label}</span>
        <button className="loop-icon-btn" onClick={onClose} style={{ marginLeft: "auto" }}>
          <X size={14} />
        </button>
      </div>
      <div className="loop-config-panel-body">
        <label className="loop-config-field">
          <span className="loop-config-label">标签</span>
          <input
            className="loop-config-input"
            value={label}
            onChange={(e) => onLabelChange(e.target.value)}
          />
        </label>
        <div className="loop-config-divider" />
        <Suspense fallback={<div className="loop-config-placeholder">加载配置…</div>}>
          {ConfigForm && <ConfigForm config={config} onChange={onConfigChange} />}
        </Suspense>
      </div>
    </div>
  );
}
