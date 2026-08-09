import { TextField, NumberField, SelectField, cfgStr, cfgNum } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

const RETRIEVAL_MODE_OPTIONS = [
  { value: "semantic", label: "语义检索 (Embedding)" },
  { value: "keyword", label: "关键词检索 (BM25)" },
  { value: "hybrid", label: "混合检索" },
];

export default function KnowledgeRetrievalConfig({ config, onChange }: ConfigProps) {
  return (
    <>
      <TextField
        label="查询文本"
        value={cfgStr(config, "query_template")}
        onChange={(v) => onChange({ ...config, query_template: v })}
        placeholder="输入检索查询…"
        multiline
        hint="支持 {{var}} 引用上游变量"
      />
      <TextField
        label="知识库路径"
        value={cfgStr(config, "knowledge_path")}
        onChange={(v) => onChange({ ...config, knowledge_path: v })}
        placeholder="文件目录、URL 或知识库 ID"
      />
      <SelectField
        label="检索模式"
        value={cfgStr(config, "retrieval_mode", "hybrid")}
        onChange={(v) => onChange({ ...config, retrieval_mode: v })}
        options={RETRIEVAL_MODE_OPTIONS}
      />
      <NumberField
        label="召回数量 (Top-K)"
        value={cfgNum(config, "top_k")}
        onChange={(v) => onChange({ ...config, top_k: v })}
        min={1}
        max={50}
        placeholder="5"
      />
      <NumberField
        label="相似度阈值"
        value={cfgNum(config, "score_threshold")}
        onChange={(v) => onChange({ ...config, score_threshold: v })}
        min={0}
        max={1}
        step={0.05}
        placeholder="0.5"
      />
    </>
  );
}
