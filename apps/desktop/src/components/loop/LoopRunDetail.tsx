import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowLeft,
  CheckCircle2,
  XCircle,
  Loader2,
  AlertTriangle,
} from "lucide-react";
import {
  ChevronDown as ChevronDownData,
  ChevronRight as ChevronRightData,
} from "lucide";
import type { LoopRunDto, LoopStepLogDto } from "./loopTypes";
import { MorphToggleIcon } from "../icons/MorphIcon";

interface Props {
  runId: string;
  onBack: () => void;
}

function formatDuration(start: string, end: string | null): string {
  if (!end) return "运行中…";
  const ms = new Date(end).getTime() - new Date(start).getTime();
  if (ms < 1000) return `${ms}ms`;
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs}s`;
  return `${Math.floor(secs / 60)}m ${secs % 60}s`;
}

function formatAbsoluteTime(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function statusDotColor(status: string): string {
  switch (status) {
    case "success":
      return "#22c55e";
    case "failure":
    case "failed":
      return "#ef4444";
    case "running":
      return "#3b82f6";
    case "skipped":
      return "#6b7280";
    default:
      return "#6b7280";
  }
}

function RunStatusBadge({ status }: { status: string }) {
  switch (status) {
    case "success":
      return (
        <span className="loop-run-badge loop-run-badge--success">
          <CheckCircle2 size={12} /> 成功
        </span>
      );
    case "failure":
    case "failed":
      return (
        <span className="loop-run-badge loop-run-badge--failure">
          <XCircle size={12} /> 失败
        </span>
      );
    case "running":
      return (
        <span className="loop-run-badge loop-run-badge--running">
          <Loader2 size={12} className="loop-spin" /> 运行中
        </span>
      );
    default:
      return (
        <span className="loop-run-badge loop-run-badge--pending">{status}</span>
      );
  }
}

function StepCard({ step }: { step: LoopStepLogDto }) {
  const [expanded, setExpanded] = useState(false);
  const hasDetail = step.output || step.error;

  return (
    <div className="loop-run-step">
      <div
        className="loop-run-step-dot"
        style={{ backgroundColor: statusDotColor(step.status) }}
      />
      <div className="loop-run-step-info">
        <div
          className={`loop-run-step-header${hasDetail ? " loop-run-step-header--clickable" : ""}`}
          onClick={() => hasDetail && setExpanded(!expanded)}
        >
          <div className="loop-run-step-label">
            <span className="loop-run-step-name">{step.node_label}</span>
            <span className="loop-run-step-type">{step.node_type}</span>
          </div>
          <div className="loop-run-step-right">
            <span className="loop-run-step-duration">
              {formatDuration(step.started_at, step.finished_at)}
            </span>
            {hasDetail ? (
              <MorphToggleIcon
                active={expanded}
                activeIcon={ChevronDownData}
                inactiveIcon={ChevronRightData}
                size={14}
                aria-hidden
              />
            ) : null}
          </div>
        </div>

        {expanded && hasDetail && (
          <div className="loop-run-step-output">
            {step.error && (
              <div className="loop-run-step-error">
                <AlertTriangle size={12} />
                <pre>{step.error}</pre>
              </div>
            )}
            {step.output && <pre>{step.output}</pre>}
          </div>
        )}
      </div>
    </div>
  );
}

export default function LoopRunDetail({ runId, onBack }: Props) {
  const [run, setRun] = useState<LoopRunDto | null>(null);
  const [steps, setSteps] = useState<LoopStepLogDto[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [outputExpanded, setOutputExpanded] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [runData, stepData] = await Promise.all([
        invoke<LoopRunDto>("get_loop_run", { runId }),
        invoke<LoopStepLogDto[]>("list_loop_step_logs", { runId }),
      ]);
      setRun(runData);
      setSteps(stepData);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [runId]);

  useEffect(() => {
    void load();
  }, [load]);

  if (loading) {
    return (
      <div className="loop-run-detail">
        <div className="loop-empty">Loading…</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="loop-run-detail">
        <div
          className="loop-empty"
          style={{ color: "var(--ink-error, #ef4444)" }}
        >
          {error}
        </div>
      </div>
    );
  }

  if (!run) {
    return (
      <div className="loop-run-detail">
        <div className="loop-empty">运行记录不存在</div>
      </div>
    );
  }

  const truncatedId = run.id.length > 12 ? run.id.slice(0, 12) + "…" : run.id;

  return (
    <div className="loop-run-detail">
      <div className="loop-run-detail-header">
        <button className="loop-icon-btn" title="返回" onClick={onBack}>
          <ArrowLeft size={16} />
        </button>
        <RunStatusBadge status={run.status} />
        <span className="loop-run-detail-id" title={run.id}>
          {truncatedId}
        </span>
        <span className="loop-run-detail-duration">
          {formatDuration(run.started_at, run.finished_at)}
        </span>
      </div>

      <div className="loop-run-detail-summary">
        <div className="loop-run-detail-row">
          <span className="loop-run-detail-label">触发方式</span>
          <span>{run.trigger_type}</span>
        </div>
        <div className="loop-run-detail-row">
          <span className="loop-run-detail-label">开始时间</span>
          <span>{formatAbsoluteTime(run.started_at)}</span>
        </div>
        {run.finished_at && (
          <div className="loop-run-detail-row">
            <span className="loop-run-detail-label">结束时间</span>
            <span>{formatAbsoluteTime(run.finished_at)}</span>
          </div>
        )}
        <div className="loop-run-detail-row">
          <span className="loop-run-detail-label">执行步数</span>
          <span>{run.node_count}</span>
        </div>
        {run.error && (
          <div className="loop-run-detail-row loop-run-detail-row--error">
            <span className="loop-run-detail-label">错误信息</span>
            <span className="loop-run-detail-error-text">{run.error}</span>
          </div>
        )}
      </div>

      <div className="loop-run-steps">
        <h4 className="loop-run-steps-title">执行步骤</h4>
        {steps.length === 0 && (
          <div className="loop-empty">
            <p>暂无步骤记录</p>
          </div>
        )}
        {steps.map((step) => (
          <StepCard key={step.id} step={step} />
        ))}
      </div>

      {run.output && (
        <div className="loop-run-output-section">
          <div
            className="loop-run-output-header"
            onClick={() => setOutputExpanded(!outputExpanded)}
          >
            <MorphToggleIcon
              active={outputExpanded}
              activeIcon={ChevronDownData}
              inactiveIcon={ChevronRightData}
              size={14}
              aria-hidden
            />
            <span>最终输出</span>
          </div>
          {outputExpanded && (
            <pre className="loop-run-output-content">{run.output}</pre>
          )}
        </div>
      )}
    </div>
  );
}
