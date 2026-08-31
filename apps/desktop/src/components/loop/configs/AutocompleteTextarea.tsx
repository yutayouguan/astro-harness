/**
 * 模板变量自动补全 textarea —— 输入 {{ 时弹出上游变量选择器。
 *
 * 用法：替代 <textarea>，传入 upstream 即可激活补全。
 * 也可以作为 <input> 使用（multiline=false）。
 */

import { useState, useRef, useCallback, useEffect } from "react";
import type { UpstreamOutput } from "./upstreamOutputs";
import { varRef } from "./upstreamOutputs";

interface Props {
  value: string;
  onChange: (v: string) => void;
  upstream?: UpstreamOutput[];
  placeholder?: string;
  className?: string;
  rows?: number;
  multiline?: boolean;
}

interface SuggestionItem {
  nodeLabel: string;
  fieldKey: string;
  fieldLabel: string;
  ref: string;
}

export default function AutocompleteTextarea({
  value,
  onChange,
  upstream,
  placeholder,
  className,
  rows = 4,
  multiline = true,
}: Props) {
  const [suggestions, setSuggestions] = useState<SuggestionItem[]>([]);
  const [selectedIdx, setSelectedIdx] = useState(0);
  const [triggerPos, setTriggerPos] = useState<number | null>(null);
  const [dropdownPos, setDropdownPos] = useState<{
    top: number;
    left: number;
  } | null>(null);
  const inputRef = useRef<HTMLTextAreaElement | HTMLInputElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);

  const allSuggestions: SuggestionItem[] = (upstream ?? []).flatMap((u) =>
    u.fields.map((f) => ({
      nodeLabel: u.nodeLabel,
      fieldKey: f.key,
      fieldLabel: f.label,
      ref: varRef(u.nodeLabel, f.key),
    })),
  );

  const checkTrigger = useCallback(() => {
    const el = inputRef.current;
    if (!el || allSuggestions.length === 0) return;

    const pos = el.selectionStart ?? 0;
    const before = value.slice(0, pos);

    // 找最近的 {{ 且之后没有 }}
    const lastOpen = before.lastIndexOf("{{");
    if (lastOpen === -1) {
      setSuggestions([]);
      setTriggerPos(null);
      return;
    }

    const afterOpen = before.slice(lastOpen + 2);
    if (afterOpen.includes("}}")) {
      setSuggestions([]);
      setTriggerPos(null);
      return;
    }

    // 搜索词
    const query = afterOpen.toLowerCase();
    const filtered = query
      ? allSuggestions.filter(
          (s) =>
            s.nodeLabel.toLowerCase().includes(query) ||
            s.fieldKey.toLowerCase().includes(query) ||
            s.fieldLabel.toLowerCase().includes(query),
        )
      : allSuggestions;

    if (filtered.length === 0) {
      setSuggestions([]);
      setTriggerPos(null);
      return;
    }

    setTriggerPos(lastOpen);
    setSuggestions(filtered);
    setSelectedIdx(0);

    // 计算下拉位置（基于 textarea 的光标位置近似）
    const rect = el.getBoundingClientRect();
    // 简单的近似定位：textarea 底部
    setDropdownPos({
      top: rect.bottom + 4,
      left: rect.left,
    });
  }, [value, allSuggestions]);

  const insertSuggestion = useCallback(
    (item: SuggestionItem) => {
      if (triggerPos === null) return;
      const el = inputRef.current;
      const cursorPos = el?.selectionStart ?? value.length;
      const before = value.slice(0, triggerPos);
      const after = value.slice(cursorPos);
      const inserted = item.ref + (after.startsWith("}}") ? "" : "}}");
      const suffix = after.startsWith("}}") ? after : after;
      const newValue = before + inserted + suffix;
      onChange(newValue);
      setSuggestions([]);
      setTriggerPos(null);

      // 恢复光标位置
      requestAnimationFrame(() => {
        if (el) {
          const newPos =
            before.length + inserted.length + (after.startsWith("}}") ? 2 : 0);
          el.selectionStart = el.selectionEnd = newPos;
          el.focus();
        }
      });
    },
    [triggerPos, value, onChange],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (suggestions.length === 0) return;

      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIdx((i) => (i + 1) % suggestions.length);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIdx(
          (i) => (i - 1 + suggestions.length) % suggestions.length,
        );
      } else if (e.key === "Enter" || e.key === "Tab") {
        if (suggestions[selectedIdx]) {
          e.preventDefault();
          insertSuggestion(suggestions[selectedIdx]);
        }
      } else if (e.key === "Escape") {
        setSuggestions([]);
        setTriggerPos(null);
      }
    },
    [suggestions, selectedIdx, insertSuggestion],
  );

  // 点击外部关闭
  useEffect(() => {
    if (suggestions.length === 0) return;
    function handleClick(e: MouseEvent) {
      if (
        dropdownRef.current &&
        !dropdownRef.current.contains(e.target as Node) &&
        inputRef.current &&
        !inputRef.current.contains(e.target as Node)
      ) {
        setSuggestions([]);
        setTriggerPos(null);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [suggestions.length]);

  const sharedProps = {
    ref: inputRef as React.RefObject<HTMLTextAreaElement & HTMLInputElement>,
    className:
      className ?? (multiline ? "loop-config-textarea" : "loop-config-input"),
    value,
    onChange: (
      e: React.ChangeEvent<HTMLTextAreaElement | HTMLInputElement>,
    ) => {
      onChange(e.target.value);
    },
    onKeyUp: checkTrigger,
    onClick: checkTrigger,
    onKeyDown: handleKeyDown,
    placeholder,
  };

  return (
    <div className="ac-wrap">
      {multiline ? (
        <textarea {...sharedProps} rows={rows} />
      ) : (
        <input {...sharedProps} />
      )}
      {suggestions.length > 0 && dropdownPos && (
        <div
          ref={dropdownRef}
          className="ac-dropdown"
          style={{ top: dropdownPos.top, left: dropdownPos.left }}
        >
          {suggestions.map((item, i) => (
            <button
              key={`${item.nodeLabel}.${item.fieldKey}`}
              className={`ac-item${i === selectedIdx ? " is-selected" : ""}`}
              onMouseEnter={() => setSelectedIdx(i)}
              onClick={() => insertSuggestion(item)}
              type="button"
            >
              <span className="ac-item-node">{item.nodeLabel}</span>
              <span className="ac-item-dot">·</span>
              <span className="ac-item-field">{item.fieldLabel}</span>
              <code className="ac-item-ref">{item.ref}</code>
            </button>
          ))}
          <div className="ac-hint">↑↓ 选择 · Enter 插入 · Esc 关闭</div>
        </div>
      )}
    </div>
  );
}
