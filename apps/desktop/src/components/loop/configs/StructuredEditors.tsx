/**
 * 结构化编辑器 — 替代手写 JSON，所有节点配置面板复用。
 *
 * - KeyValueEditor: key-value 行编辑器（请求头、术语表、输入映射、字段赋值等）
 * - TagInput: 标签/芯片输入（情感标签、自定义分类等）
 * - RowListEditor: 多行结构化列表（输出字段、参数 schema、分类列表等）
 */

import { useState, useCallback } from "react";
import { Plus, X } from "lucide-react";

// ---------------------------------------------------------------------------
// KeyValueEditor — key=value 行编辑器
// ---------------------------------------------------------------------------

export interface KvEntry {
  key: string;
  value: string;
}

interface KeyValueEditorProps {
  label: string;
  value: KvEntry[];
  onChange: (v: KvEntry[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  hint?: string;
  maxRows?: number;
}

export function KeyValueEditor({
  label,
  value,
  onChange,
  keyPlaceholder = "键",
  valuePlaceholder = "值",
  hint,
  maxRows,
}: KeyValueEditorProps) {
  const add = useCallback(() => {
    if (maxRows && value.length >= maxRows) return;
    onChange([...value, { key: "", value: "" }]);
  }, [value, onChange, maxRows]);

  const remove = useCallback(
    (idx: number) => onChange(value.filter((_, i) => i !== idx)),
    [value, onChange],
  );

  const update = useCallback(
    (idx: number, field: "key" | "value", v: string) => {
      const next = [...value];
      next[idx] = { ...next[idx], [field]: v };
      onChange(next);
    },
    [value, onChange],
  );

  return (
    <div className="loop-config-field">
      <span className="loop-config-label">{label}</span>
      {value.length > 0 && (
        <div className="se-rows">
          {value.map((entry, i) => (
            <div key={i} className="se-kv-row">
              <input
                className="loop-config-input se-kv-key"
                value={entry.key}
                onChange={(e) => update(i, "key", e.target.value)}
                placeholder={keyPlaceholder}
              />
              <span className="se-kv-sep">=</span>
              <input
                className="loop-config-input se-kv-val"
                value={entry.value}
                onChange={(e) => update(i, "value", e.target.value)}
                placeholder={valuePlaceholder}
              />
              <button
                className="se-remove"
                onClick={() => remove(i)}
                type="button"
              >
                <X size={12} />
              </button>
            </div>
          ))}
        </div>
      )}
      <button
        className="se-add-btn"
        onClick={add}
        disabled={!!(maxRows && value.length >= maxRows)}
        type="button"
      >
        <Plus size={12} />
        <span>添加</span>
      </button>
      {hint && <span className="loop-config-hint">{hint}</span>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// TagInput — 标签/芯片输入
// ---------------------------------------------------------------------------

interface TagInputProps {
  label: string;
  value: string[];
  onChange: (v: string[]) => void;
  placeholder?: string;
  hint?: string;
}

export function TagInput({
  label,
  value,
  onChange,
  placeholder = "回车添加标签",
  hint,
}: TagInputProps) {
  const [input, setInput] = useState("");

  const add = useCallback(() => {
    const tag = input.trim();
    if (tag && !value.includes(tag)) {
      onChange([...value, tag]);
    }
    setInput("");
  }, [input, value, onChange]);

  const remove = useCallback(
    (idx: number) => onChange(value.filter((_, i) => i !== idx)),
    [value, onChange],
  );

  return (
    <div className="loop-config-field">
      <span className="loop-config-label">{label}</span>
      {value.length > 0 && (
        <div className="se-tags">
          {value.map((tag, i) => (
            <span key={i} className="se-tag">
              <span>{tag}</span>
              <button
                className="se-tag-remove"
                onClick={() => remove(i)}
                type="button"
              >
                <X size={10} />
              </button>
            </span>
          ))}
        </div>
      )}
      <input
        className="loop-config-input"
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            add();
          }
        }}
        placeholder={placeholder}
      />
      {hint && <span className="loop-config-hint">{hint}</span>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// RowListEditor — 多列行编辑器（通用）
// ---------------------------------------------------------------------------

export interface ColumnDef {
  key: string;
  label: string;
  placeholder?: string;
  /** 列宽比例，默认 1 */
  flex?: number;
}

interface RowListEditorProps {
  label: string;
  columns: ColumnDef[];
  value: Record<string, string>[];
  onChange: (v: Record<string, string>[]) => void;
  hint?: string;
  maxRows?: number;
  addLabel?: string;
}

export function RowListEditor({
  label,
  columns,
  value,
  onChange,
  hint,
  maxRows,
  addLabel = "添加",
}: RowListEditorProps) {
  const newRow = useCallback((): Record<string, string> => {
    const row: Record<string, string> = {};
    for (const col of columns) row[col.key] = "";
    return row;
  }, [columns]);

  const add = useCallback(() => {
    if (maxRows && value.length >= maxRows) return;
    onChange([...value, newRow()]);
  }, [value, onChange, maxRows, newRow]);

  const remove = useCallback(
    (idx: number) => onChange(value.filter((_, i) => i !== idx)),
    [value, onChange],
  );

  const update = useCallback(
    (idx: number, key: string, v: string) => {
      const next = [...value];
      next[idx] = { ...next[idx], [key]: v };
      onChange(next);
    },
    [value, onChange],
  );

  return (
    <div className="loop-config-field">
      <span className="loop-config-label">{label}</span>
      {value.length > 0 && (
        <div className="se-rows">
          {/* 表头 */}
          <div className="se-row-header">
            {columns.map((col) => (
              <span
                key={col.key}
                className="se-col-label"
                style={{ flex: col.flex ?? 1 }}
              >
                {col.label}
              </span>
            ))}
            <span className="se-col-action" />
          </div>
          {/* 数据行 */}
          {value.map((row, i) => (
            <div key={i} className="se-row">
              {columns.map((col) => (
                <input
                  key={col.key}
                  className="loop-config-input se-cell"
                  style={{ flex: col.flex ?? 1 }}
                  value={row[col.key] ?? ""}
                  onChange={(e) => update(i, col.key, e.target.value)}
                  placeholder={col.placeholder}
                />
              ))}
              <button
                className="se-remove"
                onClick={() => remove(i)}
                type="button"
              >
                <X size={12} />
              </button>
            </div>
          ))}
        </div>
      )}
      <button
        className="se-add-btn"
        onClick={add}
        disabled={!!(maxRows && value.length >= maxRows)}
        type="button"
      >
        <Plus size={12} />
        <span>{addLabel}</span>
      </button>
      {hint && <span className="loop-config-hint">{hint}</span>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// JSON ↔ 结构化互转工具
// ---------------------------------------------------------------------------

/** JSON 字符串 → KvEntry[]，兼容 {"k":"v"} 和 [{"key":"k","value":"v"}] */
export function jsonToKvEntries(raw: unknown): KvEntry[] {
  if (Array.isArray(raw)) {
    return raw
      .filter(
        (e): e is KvEntry => typeof e === "object" && e !== null && "key" in e,
      )
      .map((e) => ({ key: String(e.key ?? ""), value: String(e.value ?? "") }));
  }
  if (typeof raw === "string" && raw.trim()) {
    try {
      const parsed = JSON.parse(raw);
      if (
        typeof parsed === "object" &&
        parsed !== null &&
        !Array.isArray(parsed)
      ) {
        return Object.entries(parsed).map(([k, v]) => ({
          key: k,
          value: String(v),
        }));
      }
      if (Array.isArray(parsed)) return jsonToKvEntries(parsed);
    } catch {
      /* ignore */
    }
  }
  if (typeof raw === "object" && raw !== null && !Array.isArray(raw)) {
    return Object.entries(raw as Record<string, unknown>).map(([k, v]) => ({
      key: k,
      value: String(v ?? ""),
    }));
  }
  return [];
}

/** KvEntry[] → JSON 对象 */
export function kvEntriesToObj(entries: KvEntry[]): Record<string, string> {
  const obj: Record<string, string> = {};
  for (const e of entries) {
    if (e.key.trim()) obj[e.key.trim()] = e.value;
  }
  return obj;
}

/** JSON 字符串 → string[]，兼容 ["a","b"] 和 "a,b" */
export function jsonToStringArray(raw: unknown): string[] {
  if (Array.isArray(raw))
    return raw.filter((x): x is string => typeof x === "string");
  if (typeof raw === "string" && raw.trim()) {
    try {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) return parsed.map(String);
    } catch {
      /* ignore */
    }
    return raw
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
  }
  return [];
}

/** JSON 字符串 → Record<string, string>[]，兼容 [{...}] 和 JSON 字符串 */
export function jsonToRowList(raw: unknown): Record<string, string>[] {
  if (Array.isArray(raw)) {
    return raw
      .filter(
        (e): e is Record<string, unknown> =>
          typeof e === "object" && e !== null,
      )
      .map((e) => {
        const row: Record<string, string> = {};
        for (const [k, v] of Object.entries(e)) row[k] = String(v ?? "");
        return row;
      });
  }
  if (typeof raw === "string" && raw.trim()) {
    try {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) return jsonToRowList(parsed);
    } catch {
      /* ignore */
    }
  }
  return [];
}
