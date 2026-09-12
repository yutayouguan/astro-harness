/** 通用配置字段组件 — 所有节点配置表单复用 */

import { useState, useRef, useEffect, useCallback } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FolderOpen, Variable, X, Loader2 } from "lucide-react";
import { AIActionIcon } from "../../icons/AIActionIcon";
import { Eye as EyeData, EyeOff as EyeOffData } from "lucide";
import { MorphToggleIcon } from "../../icons/MorphIcon";
import type { UpstreamOutput, MediaType } from "./upstreamOutputs";
import { varRef } from "./upstreamOutputs";
import AutocompleteTextarea from "./AutocompleteTextarea";

interface TextFieldProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  multiline?: boolean;
  hint?: string;
  /** 传入上游输出时，输入 {{ 自动弹出变量补全 */
  upstream?: UpstreamOutput[];
}

export function TextField({
  label,
  value,
  onChange,
  placeholder,
  multiline,
  hint,
  upstream,
}: TextFieldProps) {
  const hasUpstream = upstream && upstream.length > 0;
  return (
    <label className="loop-config-field">
      <span className="loop-config-label">{label}</span>
      {hasUpstream ? (
        <AutocompleteTextarea
          value={value}
          onChange={onChange}
          upstream={upstream}
          placeholder={placeholder}
          multiline={multiline}
          rows={multiline ? 4 : undefined}
        />
      ) : multiline ? (
        <textarea
          className="loop-config-textarea"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          rows={4}
        />
      ) : (
        <input
          className="loop-config-input"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
        />
      )}
      {hint && <span className="loop-config-hint">{hint}</span>}
    </label>
  );
}

interface NumberFieldProps {
  label: string;
  value: number | undefined;
  onChange: (v: number | undefined) => void;
  placeholder?: string;
  min?: number;
  max?: number;
  step?: number;
  hint?: string;
}

export function NumberField({
  label,
  value,
  onChange,
  placeholder,
  min,
  max,
  step,
  hint,
}: NumberFieldProps) {
  return (
    <label className="loop-config-field">
      <span className="loop-config-label">{label}</span>
      <input
        className="loop-config-input"
        type="number"
        value={value ?? ""}
        onChange={(e) =>
          onChange(e.target.value ? Number(e.target.value) : undefined)
        }
        placeholder={placeholder}
        min={min}
        max={max}
        step={step}
      />
      {hint && <span className="loop-config-hint">{hint}</span>}
    </label>
  );
}

interface SelectFieldProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
  hint?: string;
}

export function SelectField({
  label,
  value,
  onChange,
  options,
  hint,
}: SelectFieldProps) {
  return (
    <label className="loop-config-field">
      <span className="loop-config-label">{label}</span>
      <select
        className="loop-config-select"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
      {hint && <span className="loop-config-hint">{hint}</span>}
    </label>
  );
}

interface ToggleFieldProps {
  label: string;
  value: boolean;
  onChange: (v: boolean) => void;
  hint?: string;
}

export function ToggleField({
  label,
  value,
  onChange,
  hint,
}: ToggleFieldProps) {
  return (
    <label className="loop-config-field loop-config-field--row">
      <span className="loop-config-label">{label}</span>
      <input
        type="checkbox"
        className="loop-config-checkbox"
        checked={value}
        onChange={(e) => onChange(e.target.checked)}
      />
      {hint && <span className="loop-config-hint">{hint}</span>}
    </label>
  );
}

interface SectionProps {
  title: string;
  children: ReactNode;
}

export function Section({ title, children }: SectionProps) {
  return (
    <div className="loop-config-section">
      <div className="loop-config-section-title">{title}</div>
      {children}
    </div>
  );
}

/** 从节点 config 对象读取字段值 */
export function cfgStr(
  config: Record<string, unknown>,
  key: string,
  fallback = "",
): string {
  const v = config[key];
  return typeof v === "string" ? v : fallback;
}

export function cfgNum(
  config: Record<string, unknown>,
  key: string,
): number | undefined {
  const v = config[key];
  return typeof v === "number" ? v : undefined;
}

export function cfgBool(
  config: Record<string, unknown>,
  key: string,
  fallback = false,
): boolean {
  const v = config[key];
  return typeof v === "boolean" ? v : fallback;
}

export function cfgStrArray(
  config: Record<string, unknown>,
  key: string,
): string[] {
  const v = config[key];
  if (Array.isArray(v))
    return v.filter((x): x is string => typeof x === "string");
  if (typeof v === "string" && v.trim()) {
    try {
      const arr = JSON.parse(v);
      if (Array.isArray(arr)) return arr;
    } catch {
      /* ignore */
    }
  }
  return [];
}

// ---------------------------------------------------------------------------
// AI 辅助文本字段 — 润色 / 生成
// ---------------------------------------------------------------------------

interface AiAssistFieldProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  hint?: string;
  /** 描述字段用途（如"视频生成提示词"），用于 AI 生成上下文 */
  task: string;
  /** 辅助模型 provider_id（可选，缺省用全局默认） */
  aiProviderId?: string;
  /** 辅助模型名称（可选） */
  aiModel?: string;
  multiline?: boolean;
  /** 传入上游输出时，输入 {{ 自动弹出变量补全 */
  upstream?: UpstreamOutput[];
}

/** 带 AI 润色/生成按钮的文本输入 */
export function AiAssistField({
  label,
  value,
  onChange,
  placeholder,
  hint,
  task,
  aiProviderId,
  aiModel,
  multiline = true,
  upstream,
}: AiAssistFieldProps) {
  const [loading, setLoading] = useState(false);

  const handleAi = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<string>("loop_ai_polish", {
        text: value,
        task,
        providerId: aiProviderId || null,
        model: aiModel || null,
      });
      if (result && result.trim()) onChange(result.trim());
    } catch (e) {
      console.error("AI assist error:", e);
    } finally {
      setLoading(false);
    }
  }, [value, task, aiProviderId, aiModel, onChange]);

  return (
    <div className="loop-config-field">
      <div className="loop-config-label-row">
        <span className="loop-config-label">{label}</span>
        <button
          className={`loop-config-ai-btn${loading ? " is-loading" : ""}`}
          onClick={handleAi}
          disabled={loading}
          title={value.trim() ? "AI 润色" : "AI 生成"}
          type="button"
        >
          {loading ? (
            <Loader2 size={13} className="loop-spin" />
          ) : (
            <AIActionIcon size={13} />
          )}
          <span>{value.trim() ? "润色" : "AI 生成"}</span>
        </button>
      </div>
      {upstream && upstream.length > 0 ? (
        <AutocompleteTextarea
          value={value}
          onChange={onChange}
          upstream={upstream}
          placeholder={placeholder}
          multiline={multiline}
          rows={multiline ? 4 : undefined}
        />
      ) : multiline ? (
        <textarea
          className="loop-config-textarea"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          rows={4}
        />
      ) : (
        <input
          className="loop-config-input"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
        />
      )}
      {hint && <span className="loop-config-hint">{hint}</span>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// 密码字段
// ---------------------------------------------------------------------------

interface PasswordFieldProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  hint?: string;
}

export function PasswordField({
  label,
  value,
  onChange,
  placeholder,
  hint,
}: PasswordFieldProps) {
  const [visible, setVisible] = useState(false);
  return (
    <div className="loop-config-field">
      <span className="loop-config-label">{label}</span>
      <div className="loop-config-file-row">
        <input
          className="loop-config-input loop-config-file-input"
          type={visible ? "text" : "password"}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          autoComplete="off"
        />
        <button
          className="loop-config-file-btn"
          onClick={() => setVisible((v) => !v)}
          title={visible ? "隐藏" : "显示"}
          type="button"
        >
          <MorphToggleIcon
            active={visible}
            activeIcon={EyeOffData}
            inactiveIcon={EyeData}
            size={14}
          />
        </button>
      </div>
      {hint && <span className="loop-config-hint">{hint}</span>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// 文件选择器 — 文件浏览 + 上游变量选择
// ---------------------------------------------------------------------------

const FILE_FILTERS: Record<string, { name: string; extensions: string[] }[]> = {
  image: [
    {
      name: "图片",
      extensions: ["jpg", "jpeg", "png", "webp", "heic", "heif", "gif", "bmp"],
    },
  ],
  video: [{ name: "视频", extensions: ["mp4", "mov", "webm", "avi", "mkv"] }],
  audio: [
    { name: "音频", extensions: ["mp3", "wav", "aac", "flac", "ogg", "m4a"] },
  ],
};

async function openFileDialog(accept?: string): Promise<string | null> {
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const filters = accept ? FILE_FILTERS[accept] : undefined;
    const result = await open({ multiple: false, filters });
    if (typeof result === "string") return result;
    if (result && typeof (result as { path?: string }).path === "string")
      return (result as { path: string }).path;
    return null;
  } catch {
    return null;
  }
}

async function openMultiFileDialog(accept?: string): Promise<string[]> {
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const filters = accept ? FILE_FILTERS[accept] : undefined;
    const result = await open({ multiple: true, filters });
    if (Array.isArray(result)) {
      return result
        .map((r) =>
          typeof r === "string" ? r : ((r as { path?: string }).path ?? ""),
        )
        .filter(Boolean);
    }
    if (typeof result === "string") return [result];
    return [];
  } catch {
    return [];
  }
}

/** 上游变量下拉菜单 */
function VarDropdown({
  upstream,
  accept,
  onSelect,
  onClose,
}: {
  upstream: UpstreamOutput[];
  accept?: MediaType;
  onSelect: (ref: string) => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [onClose]);

  const filtered = upstream
    .map((u) => ({
      ...u,
      fields: u.fields.filter(
        (f) =>
          !accept ||
          accept === "any" ||
          f.mediaType === accept ||
          f.mediaType === "any" ||
          f.mediaType === "path",
      ),
    }))
    .filter((u) => u.fields.length > 0);

  if (filtered.length === 0) {
    return (
      <div ref={ref} className="loop-var-dropdown">
        <div className="loop-var-empty">暂无可用的上游输出</div>
      </div>
    );
  }

  return (
    <div ref={ref} className="loop-var-dropdown">
      {filtered.map((u) => (
        <div key={u.nodeId} className="loop-var-group">
          <div className="loop-var-group-label">{u.nodeLabel}</div>
          {u.fields.map((f) => (
            <button
              key={f.key}
              className="loop-var-item"
              onClick={() => {
                onSelect(varRef(u.nodeLabel, f.key));
                onClose();
              }}
            >
              <span className="loop-var-item-label">{f.label}</span>
              <code className="loop-var-item-ref">{`{{${u.nodeLabel}.${f.key}}}`}</code>
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}

interface FilePathFieldProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  hint?: string;
  /** 文件类型过滤：image / video / audio */
  accept?: string;
  /** 上游节点输出（用于变量选择） */
  upstream?: UpstreamOutput[];
}

/** 文件路径选择器 — 输入框 + 浏览文件 + 引用上游变量 */
export function FilePathField({
  label,
  value,
  onChange,
  placeholder,
  hint,
  accept,
  upstream,
}: FilePathFieldProps) {
  const [showVars, setShowVars] = useState(false);

  const handleBrowse = useCallback(async () => {
    const path = await openFileDialog(accept);
    if (path) onChange(path);
  }, [accept, onChange]);

  const handleClose = useCallback(() => setShowVars(false), []);

  return (
    <div className="loop-config-field">
      <span className="loop-config-label">{label}</span>
      <div className="loop-config-file-row">
        <input
          className="loop-config-input loop-config-file-input"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder ?? "选择文件或引用上游变量…"}
          readOnly={false}
        />
        <button
          className="loop-config-file-btn"
          title="浏览文件"
          onClick={handleBrowse}
          type="button"
        >
          <FolderOpen size={14} />
        </button>
        {upstream && upstream.length > 0 && (
          <div className="loop-config-file-var-wrap">
            <button
              className={`loop-config-file-btn${showVars ? " is-active" : ""}`}
              title="引用上游变量"
              onClick={() => setShowVars((v) => !v)}
              type="button"
            >
              <Variable size={14} />
            </button>
            {showVars && (
              <VarDropdown
                upstream={upstream}
                accept={accept as MediaType | undefined}
                onSelect={onChange}
                onClose={handleClose}
              />
            )}
          </div>
        )}
      </div>
      {hint && <span className="loop-config-hint">{hint}</span>}
    </div>
  );
}

interface FileArrayFieldProps {
  label: string;
  value: string[];
  onChange: (v: string[]) => void;
  hint?: string;
  accept?: string;
  upstream?: UpstreamOutput[];
  max?: number;
}

/** 文件数组选择器 — 列表 + 添加/移除 */
export function FileArrayField({
  label,
  value,
  onChange,
  hint,
  accept,
  upstream,
  max,
}: FileArrayFieldProps) {
  const [showVars, setShowVars] = useState(false);

  const handleBrowse = useCallback(async () => {
    const paths = await openMultiFileDialog(accept);
    if (paths.length > 0) {
      const next = [...value, ...paths];
      onChange(max ? next.slice(0, max) : next);
    }
  }, [accept, onChange, value, max]);

  const handleRemove = useCallback(
    (idx: number) => onChange(value.filter((_, i) => i !== idx)),
    [onChange, value],
  );

  const handleAddVar = useCallback(
    (ref: string) => {
      const next = [...value, ref];
      onChange(max ? next.slice(0, max) : next);
    },
    [onChange, value, max],
  );

  const handleClose = useCallback(() => setShowVars(false), []);

  return (
    <div className="loop-config-field">
      <span className="loop-config-label">
        {label}
        {max != null && (
          <span className="loop-config-hint-inline"> (最多 {max} 个)</span>
        )}
      </span>
      {value.length > 0 && (
        <div className="loop-config-file-list">
          {value.map((item, i) => (
            <div key={i} className="loop-config-file-item">
              <span className="loop-config-file-item-text" title={item}>
                {item.length > 50 ? "…" + item.slice(-48) : item}
              </span>
              <button
                className="loop-config-file-remove"
                onClick={() => handleRemove(i)}
                type="button"
              >
                <X size={12} />
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="loop-config-file-row">
        <button
          className="loop-config-file-add-btn"
          onClick={handleBrowse}
          type="button"
        >
          <FolderOpen size={13} />
          <span>添加文件</span>
        </button>
        {upstream && upstream.length > 0 && (
          <div className="loop-config-file-var-wrap">
            <button
              className={`loop-config-file-add-btn${showVars ? " is-active" : ""}`}
              onClick={() => setShowVars((v) => !v)}
              type="button"
            >
              <Variable size={13} />
              <span>引用变量</span>
            </button>
            {showVars && (
              <VarDropdown
                upstream={upstream}
                accept={accept as MediaType | undefined}
                onSelect={handleAddVar}
                onClose={handleClose}
              />
            )}
          </div>
        )}
      </div>
      {hint && <span className="loop-config-hint">{hint}</span>}
    </div>
  );
}
