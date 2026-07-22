/** 右侧节点配置面板 — 配置/运行日志 tabs + 通用底部区域 */

import { lazy, Suspense, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Braces, Trash2, Star, UserRound } from "lucide-react";
import { useConfirm } from "../../hooks/ui/DialogContext";
import * as LucideIcons from "lucide-react";
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
const CustomLoopConfig = lazy(() => import("./configs/CustomLoopConfig"));

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
  custom_loop: CustomLoopConfig,
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

interface StepLog {
  id: string;
  node_id: string;
  node_type: string;
  node_label: string;
  status: string;
  started_at: string;
  finished_at: string | null;
  output: string | null;
  error: string | null;
}

function NodeStepLogs({ nodeId }: { nodeId: string }) {
  const [logs, setLogs] = useState<StepLog[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    (async () => {
      setLoading(true);
      try {
        const runs = await invoke<{ runs: { id: string }[] }>("list_loop_runs", { workflowId: "", limit: 5 });
        const allLogs: StepLog[] = [];
        for (const run of runs.runs.slice(0, 3)) {
          try {
            const steps = await invoke<{ steps: StepLog[] }>("list_loop_step_logs", { runId: run.id });
            allLogs.push(...steps.steps.filter((s: StepLog) => s.node_id === nodeId));
          } catch { /* ignore */ }
        }
        setLogs(allLogs);
      } catch { /* ignore */ }
      setLoading(false);
    })();
  }, [nodeId]);

  if (loading) return <div className="loop-config-panel-body"><div className="loop-config-placeholder">加载中…</div></div>;
  if (logs.length === 0) return <div className="loop-config-panel-body"><div className="loop-config-placeholder">暂无此节点的运行日志</div></div>;

  return (
    <div className="loop-config-panel-body loop-config-logs">
      {logs.map((log) => (
        <div key={log.id} className={`loop-step-log loop-step-log--${log.status}`}>
          <div className="loop-step-log-header">
            <span className={`loop-step-log-dot loop-step-log-dot--${log.status}`} />
            <span className="loop-step-log-status">{log.status}</span>
            <span className="loop-step-log-time">{log.started_at}</span>
          </div>
          {log.output && (
            <pre className="loop-step-log-output">{log.output}</pre>
          )}
          {log.error && (
            <pre className="loop-step-log-error">{log.error}</pre>
          )}
        </div>
      ))}
    </div>
  );
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
  const [showJson, setShowJson] = useState(false);
  const confirm = useConfirm();
  const meta = getNodeMeta(nodeType);
  const ConfigForm = CONFIG_MAP[nodeType];
  type LucideIcon = React.ComponentType<{ size?: number; className?: string }>;
  const NodeIcon = (LucideIcons as unknown as Record<string, LucideIcon>)[meta.icon];

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
            <div className="loop-config-node-type-row">
              {NodeIcon && (
                <span className="loop-config-node-icon" style={{ color: meta.color }}>
                  <NodeIcon size={18} />
                </span>
              )}
              <span className="loop-config-node-type">{meta.label}</span>
            </div>
            <div className="loop-config-node-actions">
              <button className={`loop-icon-btn${showJson ? " is-active" : ""}`} title="查看 JSON" onClick={() => setShowJson((v) => !v)}>
                <Braces size={14} />
              </button>
              <button
                className="loop-icon-btn loop-icon-btn--danger"
                title="删除节点"
                onClick={async () => {
                  const ok = await confirm({ title: "删除节点", message: `确定删除「${label}」吗？`, confirmLabel: "删除", variant: "danger" });
                  if (ok) onDelete();
                }}
              >
                <Trash2 size={14} />
              </button>
            </div>
          </div>

          {/* ── JSON viewer ── */}
          {showJson && (
            <pre className="loop-config-json-viewer">
              {JSON.stringify(config, null, 2)}
            </pre>
          )}

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

            {/* 预设保存功能待后端支持 */}
          </div>
        </div>
      )}

      {tab === "logs" && (
        <NodeStepLogs nodeId={nodeId} />
      )}
    </div>
  );
}
