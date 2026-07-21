import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Plus,
  Play,
  Pencil,
  Trash2,
  Download,
  Upload,
  Power,
  Bot,
} from "lucide-react";
import type { LoopDto } from "./loopTypes";
import LoopEditor from "./LoopEditor";

interface Props {
  active: boolean;
  providers: { id: string; name: string; model: string; kind: string }[];
}

export default function LoopPanel({ active, providers }: Props) {
  const [loops, setLoops] = useState<LoopDto[]>([]);
  const [loading, setLoading] = useState(true);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [isEditing, setIsEditing] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<LoopDto[]>("list_loops");
      setLoops(list);
    } catch (e) {
      console.error("list_loops failed", e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (active) void refresh();
  }, [active, refresh]);

  const handleCreate = async () => {
    try {
      const created = await invoke<LoopDto>("create_loop", {
        name: "未命名创建loop",
        description: "",
      });
      setEditingId(created.id);
      setIsEditing(true);
      void refresh();
    } catch (e) {
      console.error("create_loop failed", e);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await invoke("delete_loop", { id });
      void refresh();
    } catch (e) {
      console.error("delete_loop failed", e);
    }
  };

  const handleToggleEnabled = async (id: string, enabled: boolean) => {
    try {
      await invoke("set_loop_enabled", { id, enabled });
      void refresh();
    } catch (e) {
      console.error("set_loop_enabled failed", e);
    }
  };

  const handleToggleAiCallable = async (id: string, callable: boolean) => {
    try {
      await invoke("set_loop_ai_callable", { id, callable });
      void refresh();
    } catch (e) {
      console.error("set_loop_ai_callable failed", e);
    }
  };

  const handleExport = async (id: string) => {
    try {
      const json = await invoke<string>("export_loop", { id });
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `loop-${id}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      console.error("export_loop failed", e);
    }
  };

  const handleImport = async () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      const text = await file.text();
      try {
        await invoke("import_loop", { json: text });
        void refresh();
      } catch (e) {
        console.error("import_loop failed", e);
      }
    };
    input.click();
  };

  if (isEditing) {
    return (
      <LoopEditor
        workflowId={editingId}
        providers={providers}
        onBack={() => {
          setIsEditing(false);
          setEditingId(null);
          void refresh();
        }}
      />
    );
  }

  return (
    <div className="loop-panel">
      <div className="loop-panel-header">
        <div className="loop-panel-title-area">
          <h2 className="loop-panel-title">Loop</h2>
          <p className="loop-panel-desc">
            用节点搭好一条自动化流程：手动运行、按定时 / Webhook
            自动触发，或让 code 模式的智能体直接调用。每条流程下方有「启用」和「AI
            可调用」两个开关。
          </p>
        </div>
        <div className="loop-panel-actions">
          <button className="loop-btn loop-btn--secondary" onClick={handleImport}>
            <Upload size={14} />
            <span>导入</span>
          </button>
          <button className="loop-btn loop-btn--primary" onClick={handleCreate}>
            <Plus size={14} />
            <span>新建创建loop</span>
          </button>
        </div>
      </div>

      <div className="loop-list">
        {loading && <div className="loop-empty">加载中…</div>}
        {!loading && loops.length === 0 && (
          <div className="loop-empty">
            <p>暂无 Loop 工作流</p>
            <p className="loop-empty-hint">
              点击「新建创建loop」开始搭建你的第一条自动化流程
            </p>
          </div>
        )}
        {loops.map((lp) => (
          <div key={lp.id} className="loop-card">
            <div className="loop-card-header">
              <div className="loop-card-icon">
                <Bot size={18} />
              </div>
              <div className="loop-card-info">
                <span className="loop-card-name">{lp.name}</span>
                <span className="loop-card-badge">空闲</span>
              </div>
              <div className="loop-card-actions">
                <button
                  className="loop-icon-btn"
                  title="运行一次"
                  onClick={async () => {
                    try {
                      const result = await invoke<{ run_id: string; status: string; steps_executed: number }>("run_loop", { id: lp.id });
                      console.log("run_loop result:", result);
                    } catch (e) {
                      console.error("run_loop failed", e);
                    }
                  }}
                >
                  <Play size={14} />
                </button>
                <button
                  className="loop-icon-btn"
                  title="编辑"
                  onClick={() => {
                    setEditingId(lp.id);
                    setIsEditing(true);
                  }}
                >
                  <Pencil size={14} />
                </button>
                <button
                  className="loop-icon-btn"
                  title="导出"
                  onClick={() => void handleExport(lp.id)}
                >
                  <Download size={14} />
                </button>
                <button
                  className="loop-icon-btn loop-icon-btn--danger"
                  title="删除"
                  onClick={() => void handleDelete(lp.id)}
                >
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
            <div className="loop-card-meta">
              <span>{lp.nodes.length} 个节点</span>
              <span>·</span>
              <span>上次运行: 从未</span>
            </div>
            <div className="loop-card-toggles">
              <label className="loop-toggle">
                <input
                  type="checkbox"
                  checked={lp.enabled}
                  onChange={(e) =>
                    void handleToggleEnabled(lp.id, e.target.checked)
                  }
                />
                <Power size={12} />
                <span>启用</span>
              </label>
              <label className="loop-toggle">
                <input
                  type="checkbox"
                  checked={lp.ai_callable}
                  onChange={(e) =>
                    void handleToggleAiCallable(lp.id, e.target.checked)
                  }
                />
                <Bot size={12} />
                <span>AI 可调用</span>
              </label>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
