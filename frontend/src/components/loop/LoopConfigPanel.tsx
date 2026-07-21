/** 右侧节点配置面板 — 配置/运行日志 tabs + 通用底部区域 */

import { lazy, Suspense, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { User, Braces, Trash2, Star } from "lucide-react";
import type { NodeType } from "./loopTypes";
import { getNodeMeta } from "./loopTypes";
import ErrorHandlingConfig from "./configs/ErrorHandlingConfig";

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
  nodeId: string;
  nodeType: NodeType;
  label: string;
  config: Record<string, unknown>;
  disabled: boolean;
  onLabelChange: (label: string) => void;
  onConfigChange: (config: Record<string, unknown>) => void;
  onDisabledChange: (disabled: boolean) => void;
  onDelete: () => void;
  onClose: () => void;
}

export default function LoopConfigPanel({
  nodeId,
  nodeType,
  label,
  config,
  disabled,
  onLabelChange,
  onConfigChange,
  onDisabledChange,
  onDelete,
  onClose,
}: Props) {
  const [tab, setTab] = useState<"config" | "logs">("config");
  const [presetName, setPresetName] = useState(label);
  const meta = getNodeMeta(nodeType);
  const ConfigForm = CONFIG_MAP[nodeType];

  const handleSavePreset = async () => {
    // TODO: invoke save_node_preset
    console.log("save preset:", presetName, config);
  };

  return (
    <div className="loop-config-panel">
      {/* ── Header: tabs ── */}
      <div className="loop-config-panel-tabs">
        <button
          className={`loop-config-tab${tab === "config" ? " is-active" : ""}`}
          onClick={() => setTab("config")}
        >
          配置
        </button>
        <button
          className={`loop-config-tab${tab === "logs" ? " is-active" : ""}`}
          onClick={() => setTab("logs")}
        >
          运行日志
        </button>
      </div>

      {tab === "config" && (
        <div className="loop-config-panel-scroll">
          {/* ── Node type header ── */}
          <div className="loop-config-node-header">
            <span className="loop-config-node-type">{meta.label}</span>
            <div className="loop-config-node-actions">
              <button className="loop-icon-btn" title="查看 JSON">
                <Braces size={14} />
              </button>
              <button
                className="loop-icon-btn loop-icon-btn--danger"
                title="删除节点"
                onClick={onDelete}
              >
                <Trash2 size={14} />
              </button>
            </div>
          </div>

          {/* ── Label field ── */}
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

            {/* ── Node-specific config form ── */}
            <Suspense fallback={<div className="loop-config-placeholder">加载配置…</div>}>
              {ConfigForm && <ConfigForm config={config} onChange={onConfigChange} />}
            </Suspense>

            <div className="loop-config-divider" />

            {/* ── Error handling (common to all nodes) ── */}
            <ErrorHandlingConfig config={config} onChange={onConfigChange} />

            <div className="loop-config-divider" />

            {/* ── Disabled toggle ── */}
            <label className="loop-config-field loop-config-field--row">
              <input
                type="checkbox"
                className="loop-config-checkbox-native"
                checked={disabled}
                onChange={(e) => onDisabledChange(e.target.checked)}
              />
              <span className="loop-config-label">已禁用</span>
            </label>

            <div className="loop-config-divider" />

            {/* ── Save as preset ── */}
            <div className="loop-config-preset">
              <span className="loop-config-label">保存为预设</span>
              <div className="loop-config-preset-row">
                <input
                  className="loop-config-input"
                  value={presetName}
                  onChange={(e) => setPresetName(e.target.value)}
                  placeholder={meta.label}
                />
                <button
                  className="loop-btn loop-btn--secondary loop-btn--sm"
                  onClick={() => void handleSavePreset()}
                >
                  <Star size={12} />
                  <span>保存</span>
                </button>
              </div>
              <span className="loop-config-hint">
                在「自定义」分组下新增一个带有当前配置的可复用项。
              </span>
            </div>
          </div>
        </div>
      )}

      {tab === "logs" && (
        <div className="loop-config-panel-body">
          <div className="loop-config-placeholder">
            选择一次运行后，此处显示该节点的执行日志。
          </div>
        </div>
      )}
    </div>
  );
}
