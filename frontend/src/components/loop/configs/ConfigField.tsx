/** 通用配置字段组件 — 所有节点配置表单复用 */

import type { ReactNode } from "react";

interface TextFieldProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  multiline?: boolean;
  hint?: string;
}

export function TextField({ label, value, onChange, placeholder, multiline, hint }: TextFieldProps) {
  return (
    <label className="loop-config-field">
      <span className="loop-config-label">{label}</span>
      {multiline ? (
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

export function NumberField({ label, value, onChange, placeholder, min, max, step, hint }: NumberFieldProps) {
  return (
    <label className="loop-config-field">
      <span className="loop-config-label">{label}</span>
      <input
        className="loop-config-input"
        type="number"
        value={value ?? ""}
        onChange={(e) => onChange(e.target.value ? Number(e.target.value) : undefined)}
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

export function SelectField({ label, value, onChange, options, hint }: SelectFieldProps) {
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

export function ToggleField({ label, value, onChange, hint }: ToggleFieldProps) {
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
export function cfgStr(config: Record<string, unknown>, key: string, fallback = ""): string {
  const v = config[key];
  return typeof v === "string" ? v : fallback;
}

export function cfgNum(config: Record<string, unknown>, key: string): number | undefined {
  const v = config[key];
  return typeof v === "number" ? v : undefined;
}

export function cfgBool(config: Record<string, unknown>, key: string, fallback = false): boolean {
  const v = config[key];
  return typeof v === "boolean" ? v : fallback;
}
