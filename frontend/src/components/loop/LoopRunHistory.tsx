import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CheckCircle2,
  XCircle,
  Loader2,
  X,
  RefreshCw,
} from "lucide-react";
import type { LoopRunDto } from "./loopTypes";

interface Props {
  workflowId: string;
  onSelectRun: (runId: string) => void;
  onClose: () => void;
}

function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "刚刚";
  if (mins < 60) return `${mins} 分钟前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  return `${days} 天前`;
}

function formatDuration(start: string, end: string | null): string {
  if (!end) return "运行中…";
  const ms = new Date(end).getTime() - new Date(start).getTime();
  if (ms < 1000) return `${ms}ms`;
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs}s`;
  return `${Math.floor(secs / 60)}m ${secs % 60}s`;
}

function StatusIcon({ status }: { status: string }) {
  switch (status) {
    case "success":
      return <CheckCircle2 size={16} color="#22c55e" />;
    case "failure":
    case "failed":
      return <XCircle size={16} color="#ef4444" />;
    case "running":
      return <Loader2 size={16} color="#3b82f6" className="loop-spin" />;
    default:
      return <Loader2 size={16} color="#6b7280" />;
  }
}

export default function LoopRunHistory({
  workflowId,
  onSelectRun,
  onClose,
}: Props) {
  const [runs, setRuns] = useState<LoopRunDto[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const list = await invoke<LoopRunDto[]>("list_loop_runs", {
        workflowId,
        limit: 50,
      });
      setRuns(list);
    } catch (e) {
      console.error("list_loop_runs failed", e);
    } finally {
      setLoading(false);
    }
  }, [workflowId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="loop-run-history">
      <div className="loop-run-history-header">
        <h3>运行日志</h3>
        <div className="loop-run-history-header-actions">
          <button
            className="loop-icon-btn"
            title="刷新"
            onClick={() => void refresh()}
          >
            <RefreshCw size={14} />
          </button>
          <button className="loop-icon-btn" title="关闭" onClick={onClose}>
            <X size={14} />
          </button>
        </div>
      </div>

      <div className="loop-run-history-list">
        {loading && <div className="loop-empty">加载中…</div>}

        {!loading && runs.length === 0 && (
          <div className="loop-empty">
            <p>暂无运行记录</p>
            <p className="loop-empty-hint">
              运行工作流后，执行记录将显示在此处
            </p>
          </div>
        )}

        {!loading &&
          runs.map((run) => (
            <div
              key={run.id}
              className="loop-run-card"
              onClick={() => onSelectRun(run.id)}
            >
              <div className="loop-run-card-status">
                <StatusIcon status={run.status} />
              </div>
              <div className="loop-run-card-info">
                <span className="loop-run-card-name">
                  {run.workflow_name}
                </span>
                <span className="loop-run-card-trigger">{run.trigger_type}</span>
              </div>
              <div className="loop-run-card-meta">
                <span>{relativeTime(run.started_at)}</span>
                <span className="loop-run-card-sep">·</span>
                <span>
                  {formatDuration(run.started_at, run.finished_at)}
                </span>
                <span className="loop-run-card-sep">·</span>
                <span>{run.node_count} 步</span>
              </div>
            </div>
          ))}
      </div>
    </div>
  );
}
