import { useState } from "react";
import { ChevronDown, ChevronRight, Plus } from "lucide-react";
import { TextField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import ReasoningLevelSelect from "./ReasoningLevelSelect";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function AiAgentTaskConfig({ config, onChange }: ConfigProps) {
  const [upstreamOpen, setUpstreamOpen] = useState(false);

  return (
    <>
      {/* 引用上游字段 */}
      <div className="loop-config-upstream">
        <button
          className="loop-config-upstream-toggle"
          onClick={() => setUpstreamOpen((v) => !v)}
        >
          {upstreamOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          <span>引用上游字段</span>
          <span className="loop-config-upstream-badge">可选</span>
          <span className="loop-config-upstream-add">
            <Plus size={12} />
            添加
          </span>
        </button>
        {upstreamOpen && (
          <div className="loop-config-upstream-body">
            <span className="loop-config-hint">
              默认情况下，上游的输出会自动传给本节点。只有当你想在指令里精确引用某个字段时，才需要在这里给它起个名字。
            </span>
          </div>
        )}
      </div>

      {/* 指令 */}
      <TextField
        label="指令"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        multiline
        placeholder="描述你希望智能体执行的操作。用 {{text}} 引用上一步的输出。"
        hint="不写 {{}} 时，上游的输出会自动接到指令末尾；想控制位置就用 {{}}（例如 {{text}}）。"
      />

      {/* 供应商 + 模型 */}
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
      />

      {/* 推理强度 */}
      <ReasoningLevelSelect
        value={cfgStr(config, "reasoning_level", "medium")}
        onChange={(v) => onChange({ ...config, reasoning_level: v })}
      />
    </>
  );
}
