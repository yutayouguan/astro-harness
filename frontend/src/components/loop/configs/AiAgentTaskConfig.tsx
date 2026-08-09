import { useState } from "react";
import { ChevronDown, ChevronRight, Plus, X, Variable } from "lucide-react";
import { AiAssistField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import ReasoningLevelSelect from "./ReasoningLevelSelect";
import type { UpstreamOutput } from "./upstreamOutputs";
import { varRef } from "./upstreamOutputs";

interface UpstreamFieldEntry {
  name: string;
  ref: string;
}

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
  aiProviderId?: string;
  aiModel?: string;
}

function getUpstreamFields(config: Record<string, unknown>): UpstreamFieldEntry[] {
  const raw = config.upstream_fields;
  if (Array.isArray(raw)) {
    return raw.filter(
      (e): e is UpstreamFieldEntry =>
        typeof e === "object" && e !== null && "name" in e && "ref" in e,
    );
  }
  return [];
}

export default function AiAgentTaskConfig({ config, onChange, upstreamOutputs, aiProviderId, aiModel }: ConfigProps) {
  const [upstreamOpen, setUpstreamOpen] = useState(false);
  const fields = getUpstreamFields(config);
  const upstream = upstreamOutputs ?? [];

  const updateFields = (next: UpstreamFieldEntry[]) => {
    onChange({ ...config, upstream_fields: next });
  };

  const addField = () => {
    updateFields([...fields, { name: "", ref: "" }]);
    if (!upstreamOpen) setUpstreamOpen(true);
  };

  const removeField = (idx: number) => {
    updateFields(fields.filter((_, i) => i !== idx));
  };

  const updateFieldName = (idx: number, name: string) => {
    const next = [...fields];
    next[idx] = { ...next[idx], name };
    updateFields(next);
  };

  const updateFieldRef = (idx: number, ref: string) => {
    const next = [...fields];
    next[idx] = { ...next[idx], ref };
    updateFields(next);
  };

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
          {fields.length > 0 && (
            <span className="loop-config-upstream-count">{fields.length}</span>
          )}
          <span
            className="loop-config-upstream-add"
            role="button"
            onClick={(e) => {
              e.stopPropagation();
              addField();
            }}
          >
            <Plus size={12} />
            添加
          </span>
        </button>
        {upstreamOpen && (
          <div className="loop-config-upstream-body">
            {fields.length === 0 && (
              <span className="loop-config-hint">
                默认情况下，上游的输出会自动传给本节点。点击「添加」可定义命名引用，在指令中用 {"{{名称}}"} 精确引用。
              </span>
            )}
            {fields.map((entry, idx) => (
              <UpstreamFieldRow
                key={idx}
                entry={entry}
                upstream={upstream}
                onNameChange={(name) => updateFieldName(idx, name)}
                onRefChange={(ref) => updateFieldRef(idx, ref)}
                onRemove={() => removeField(idx)}
              />
            ))}
            {fields.length > 0 && (
              <button
                className="loop-config-upstream-add-row"
                onClick={addField}
                type="button"
              >
                <Plus size={12} />
                <span>添加字段</span>
              </button>
            )}
          </div>
        )}
      </div>

      {/* 指令 */}
      <AiAssistField
        label="指令"
        value={cfgStr(config, "prompt_template")}
        onChange={(v) => onChange({ ...config, prompt_template: v })}
        multiline
        placeholder="描述你希望智能体执行的操作。用 {{text}} 引用上一步的输出。"
        hint="不写 {{}} 时，上游的输出会自动接到指令末尾；想控制位置就用 {{}}（例如 {{text}}）。"
        task="AI 智能体指令"
        aiProviderId={aiProviderId}
        aiModel={aiModel}
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

/** 单行上游字段映射 */
function UpstreamFieldRow({
  entry,
  upstream,
  onNameChange,
  onRefChange,
  onRemove,
}: {
  entry: UpstreamFieldEntry;
  upstream: UpstreamOutput[];
  onNameChange: (name: string) => void;
  onRefChange: (ref: string) => void;
  onRemove: () => void;
}) {
  const [showPicker, setShowPicker] = useState(false);

  return (
    <div className="loop-upstream-field-row">
      <input
        className="loop-config-input loop-upstream-field-name"
        value={entry.name}
        onChange={(e) => onNameChange(e.target.value)}
        placeholder="变量名"
      />
      <span className="loop-upstream-field-eq">=</span>
      <div className="loop-upstream-field-ref-wrap">
        <input
          className="loop-config-input loop-upstream-field-ref"
          value={entry.ref}
          onChange={(e) => onRefChange(e.target.value)}
          placeholder="选择或输入引用"
          readOnly={false}
        />
        {upstream.length > 0 && (
          <button
            className={`loop-config-file-btn loop-upstream-field-pick${showPicker ? " is-active" : ""}`}
            onClick={() => setShowPicker((v) => !v)}
            title="选择上游输出"
            type="button"
          >
            <Variable size={13} />
          </button>
        )}
        {showPicker && upstream.length > 0 && (
          <div className="loop-var-dropdown loop-upstream-field-dropdown">
            {upstream.map((u) => (
              <div key={u.nodeId} className="loop-var-group">
                <div className="loop-var-group-label">{u.nodeLabel}</div>
                {u.fields.map((f) => (
                  <button
                    key={f.key}
                    className="loop-var-item"
                    onClick={() => {
                      onRefChange(varRef(u.nodeLabel, f.key));
                      if (!entry.name) onNameChange(f.key);
                      setShowPicker(false);
                    }}
                  >
                    <span className="loop-var-item-label">{f.label}</span>
                    <code className="loop-var-item-ref">{varRef(u.nodeLabel, f.key)}</code>
                  </button>
                ))}
              </div>
            ))}
          </div>
        )}
      </div>
      <button
        className="loop-config-file-remove"
        onClick={onRemove}
        title="移除"
        type="button"
      >
        <X size={12} />
      </button>
    </div>
  );
}
