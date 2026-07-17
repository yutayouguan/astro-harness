/** 代码/文本文件内嵌预览：按后缀语法高亮（CodeMirror 只读）。可读盘或直接用内存内容。 */
import { useEffect, useMemo, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { EditorView } from "@codemirror/view";
import { invoke } from "@tauri-apps/api/core";
import {
  codeMirrorTheme,
  languageForFilename,
} from "../../lib/filespace/codeMirrorLanguage";
import { useTheme } from "../../hooks/app/useTheme";
import BrokenMedia from "./BrokenMedia";

type Props = {
  /** 本地绝对路径（无 source 时读取内容）。 */
  path?: string | null;
  /** 内存内容（有则优先，用于流式实时预览）。 */
  source?: string | null;
  /** 用于推断语法高亮语言；缺省时取 path 的文件名。 */
  filename?: string;
  className?: string;
  compact?: boolean;
};

function basenameOf(path: string | null | undefined): string {
  if (!path) return "";
  return path.split(/[\\/]/).pop() ?? path;
}

export default function CodeFileCard({
  path,
  source,
  filename,
  className,
  compact,
}: Props) {
  const [text, setText] = useState<string | null>(source ?? null);
  const [error, setError] = useState(false);
  const { resolved } = useTheme();

  useEffect(() => {
    if (source != null) {
      setText(source);
      setError(false);
      return;
    }
    if (!path) {
      setText(null);
      setError(true);
      return;
    }
    let cancelled = false;
    setError(false);
    void invoke<string>("read_file", { path })
      .then((content) => {
        if (!cancelled) setText(content);
      })
      .catch(() => {
        if (!cancelled) {
          setText(null);
          setError(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [path, source]);

  const name = filename ?? basenameOf(path);
  const extensions = useMemo(
    () => [
      codeMirrorTheme(resolved),
      EditorView.lineWrapping,
      EditorView.editable.of(false),
      ...languageForFilename(name),
    ],
    [resolved, name],
  );

  const openExternally = () => {
    if (!path) return;
    void invoke("open_path_externally", { path }).catch(() => {});
  };

  if (error || text == null) {
    return (
      <BrokenMedia
        path={path}
        onOpenExternally={path ? openExternally : undefined}
        className={className}
      />
    );
  }

  return (
    <div
      className={`code-file-card ${compact ? "is-compact" : ""} ${className ?? ""}`.trim()}
    >
      <CodeMirror
        className="code-file-card-cm"
        value={text}
        theme="none"
        editable={false}
        readOnly
        extensions={extensions}
        basicSetup={{
          lineNumbers: true,
          foldGutter: false,
          highlightActiveLine: false,
          highlightActiveLineGutter: false,
          bracketMatching: true,
          autocompletion: false,
          dropCursor: false,
          allowMultipleSelections: false,
          indentOnInput: false,
        }}
      />
    </div>
  );
}
