import { Plus, Trash2 } from "lucide-react";
import { Section } from "./ConfigField";

interface BranchItem {
  id: string;
  label: string;
  condition: string;
}

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

function parseBranches(raw: unknown): BranchItem[] {
  if (typeof raw === "string") {
    try {
      return JSON.parse(raw);
    } catch {
      return [];
    }
  }
  if (Array.isArray(raw)) return raw as BranchItem[];
  return [];
}

export default function MultiBranchConfig({ config, onChange }: ConfigProps) {
  const branches = parseBranches(config.branches);

  const update = (items: BranchItem[]) => {
    onChange({ ...config, branches: items });
  };

  const addBranch = () => {
    const id = `branch_${Date.now()}`;
    update([...branches, { id, label: "", condition: "" }]);
  };

  const removeBranch = (idx: number) => {
    update(branches.filter((_, i) => i !== idx));
  };

  const updateBranch = (
    idx: number,
    field: keyof BranchItem,
    value: string,
  ) => {
    const next = branches.map((b, i) =>
      i === idx ? { ...b, [field]: value } : b,
    );
    update(next);
  };

  return (
    <Section title="多路分支">
      <div className="loop-branch-editor">
        {branches.map((b, i) => (
          <div key={b.id} className="loop-branch-row">
            <input
              className="loop-config-input loop-branch-input"
              value={b.label}
              onChange={(e) => updateBranch(i, "label", e.target.value)}
              placeholder="分支名称"
            />
            <input
              className="loop-config-input loop-branch-input loop-branch-input--expr"
              value={b.condition}
              onChange={(e) => updateBranch(i, "condition", e.target.value)}
              placeholder="条件表达式（空 = 默认分支）"
            />
            <button
              className="loop-icon-btn loop-icon-btn--danger loop-branch-del"
              onClick={() => removeBranch(i)}
              title="删除分支"
            >
              <Trash2 size={12} />
            </button>
          </div>
        ))}
        <button
          className="loop-btn loop-btn--secondary loop-btn--sm"
          onClick={addBranch}
        >
          <Plus size={12} />
          <span>添加分支</span>
        </button>
      </div>
      <span className="loop-config-hint">
        每个分支对应一个输出端口。条件为空的分支是默认（兜底）分支。
      </span>
    </Section>
  );
}
