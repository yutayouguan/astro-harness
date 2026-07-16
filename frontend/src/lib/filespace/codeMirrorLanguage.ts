/** CodeMirror 语言推断与亮暗主题（工作区编辑器 / 技能预览共用）。 */
import { createTheme } from "@uiw/codemirror-themes";
import { tags as t } from "@lezer/highlight";
import { StreamLanguage } from "@codemirror/language";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { xml } from "@codemirror/lang-xml";
import { yaml } from "@codemirror/lang-yaml";
import { languages } from "@codemirror/language-data";
import type { Extension } from "@codemirror/state";
import type { ResolvedTheme } from "../../hooks/useTheme";

const mono =
  'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Monaco, Consolas, "Liberation Mono", monospace';

export const codeMirrorLightTheme = createTheme({
  theme: "light",
  settings: {
    background: "transparent",
    foreground: "#334155",
    caret: "#7c3aed",
    selection: "rgba(139, 92, 246, 0.18)",
    selectionMatch: "rgba(139, 92, 246, 0.1)",
    lineHighlight: "rgba(139, 92, 246, 0.07)",
    gutterBackground: "transparent",
    gutterForeground: "#94a3b8",
    gutterActiveForeground: "#7c3aed",
    gutterBorder: "transparent",
    fontFamily: mono,
    fontSize: "13.5px",
  },
  styles: [
    { tag: t.comment, color: "#94a3b8", fontStyle: "italic" },
    { tag: t.heading, color: "#5b21b6", fontWeight: "700" },
    { tag: t.heading1, color: "#4c1d95", fontWeight: "800", fontSize: "1.15em" },
    { tag: t.heading2, color: "#6d28d9", fontWeight: "700", fontSize: "1.08em" },
    { tag: t.heading3, color: "#7c3aed", fontWeight: "700" },
    { tag: t.strong, color: "#0f172a", fontWeight: "700" },
    { tag: t.emphasis, color: "#475569", fontStyle: "italic" },
    { tag: t.strikethrough, textDecoration: "line-through", color: "#94a3b8" },
    { tag: t.link, color: "#2563eb", textDecoration: "underline" },
    { tag: t.url, color: "#0284c7" },
    { tag: t.quote, color: "#64748b", fontStyle: "italic" },
    { tag: t.list, color: "#7c3aed" },
    { tag: t.meta, color: "#a855f7" },
    { tag: t.processingInstruction, color: "#c026d3" },
    { tag: t.keyword, color: "#7c3aed", fontWeight: "600" },
    { tag: [t.string, t.special(t.string)], color: "#059669" },
    { tag: t.number, color: "#d97706" },
    { tag: t.bool, color: "#db2777" },
    { tag: t.null, color: "#db2777" },
    { tag: t.propertyName, color: "#2563eb" },
    { tag: t.atom, color: "#0891b2" },
    { tag: t.operator, color: "#64748b" },
    { tag: t.punctuation, color: "#94a3b8" },
    { tag: t.bracket, color: "#64748b" },
    { tag: t.tagName, color: "#db2777" },
    { tag: t.attributeName, color: "#d97706" },
    { tag: t.attributeValue, color: "#059669" },
    { tag: t.className, color: "#c026d3" },
    { tag: t.typeName, color: "#0891b2" },
    { tag: t.variableName, color: "#0f172a" },
    { tag: t.definition(t.variableName), color: "#1d4ed8" },
    { tag: t.function(t.variableName), color: "#7c3aed" },
    { tag: t.monospace, color: "#0f766e", background: "rgba(13, 148, 136, 0.08)" },
    { tag: t.contentSeparator, color: "#c4b5fd" },
  ],
});

export const codeMirrorDarkTheme = createTheme({
  theme: "dark",
  settings: {
    background: "transparent",
    foreground: "#e2e8f0",
    caret: "#c4b5fd",
    selection: "rgba(167, 139, 250, 0.28)",
    selectionMatch: "rgba(167, 139, 250, 0.14)",
    lineHighlight: "rgba(167, 139, 250, 0.1)",
    gutterBackground: "transparent",
    gutterForeground: "#64748b",
    gutterActiveForeground: "#c4b5fd",
    gutterBorder: "transparent",
    fontFamily: mono,
    fontSize: "13.5px",
  },
  styles: [
    { tag: t.comment, color: "#64748b", fontStyle: "italic" },
    { tag: t.heading, color: "#ddd6fe", fontWeight: "700" },
    { tag: t.heading1, color: "#f5f3ff", fontWeight: "800", fontSize: "1.15em" },
    { tag: t.heading2, color: "#e9d5ff", fontWeight: "700", fontSize: "1.08em" },
    { tag: t.heading3, color: "#c4b5fd", fontWeight: "700" },
    { tag: t.strong, color: "#f8fafc", fontWeight: "700" },
    { tag: t.emphasis, color: "#cbd5e1", fontStyle: "italic" },
    { tag: t.strikethrough, textDecoration: "line-through", color: "#64748b" },
    { tag: t.link, color: "#93c5fd", textDecoration: "underline" },
    { tag: t.url, color: "#7dd3fc" },
    { tag: t.quote, color: "#94a3b8", fontStyle: "italic" },
    { tag: t.list, color: "#c4b5fd" },
    { tag: t.meta, color: "#e879f9" },
    { tag: t.processingInstruction, color: "#f0abfc" },
    { tag: t.keyword, color: "#c4b5fd", fontWeight: "600" },
    { tag: [t.string, t.special(t.string)], color: "#6ee7b7" },
    { tag: t.number, color: "#fbbf24" },
    { tag: t.bool, color: "#f9a8d4" },
    { tag: t.null, color: "#f9a8d4" },
    { tag: t.propertyName, color: "#93c5fd" },
    { tag: t.atom, color: "#67e8f9" },
    { tag: t.operator, color: "#94a3b8" },
    { tag: t.punctuation, color: "#64748b" },
    { tag: t.bracket, color: "#94a3b8" },
    { tag: t.tagName, color: "#f9a8d4" },
    { tag: t.attributeName, color: "#fcd34d" },
    { tag: t.attributeValue, color: "#6ee7b7" },
    { tag: t.className, color: "#e879f9" },
    { tag: t.typeName, color: "#67e8f9" },
    { tag: t.variableName, color: "#e2e8f0" },
    { tag: t.definition(t.variableName), color: "#93c5fd" },
    { tag: t.function(t.variableName), color: "#c4b5fd" },
    { tag: t.monospace, color: "#5eead4", background: "rgba(45, 212, 191, 0.1)" },
    { tag: t.contentSeparator, color: "#7c3aed" },
  ],
});

/** 按文件名后缀推断语法扩展。 */
export function languageForFilename(filename: string): Extension[] {
  const base = filename.includes("/")
    ? filename.slice(filename.lastIndexOf("/") + 1)
    : filename;
  const ext = base.includes(".")
    ? base.split(".").pop()?.toLowerCase()
    : "";
  switch (ext) {
    case "json":
      return [json()];
    case "md":
    case "markdown":
      return [
        markdown({
          base: markdownLanguage,
          codeLanguages: languages,
          addKeymap: true,
        }),
      ];
    case "js":
    case "mjs":
    case "cjs":
      return [javascript()];
    case "jsx":
      return [javascript({ jsx: true })];
    case "ts":
      return [javascript({ typescript: true })];
    case "tsx":
      return [javascript({ typescript: true, jsx: true })];
    case "rs":
      return [rust()];
    case "py":
      return [python()];
    case "css":
      return [css()];
    case "html":
    case "htm":
      return [html()];
    case "yaml":
    case "yml":
      return [yaml()];
    case "xml":
    case "svg":
      return [xml()];
    case "sh":
    case "bash":
    case "zsh":
    case "fish":
    case "ksh":
      return [StreamLanguage.define(shell)];
    default:
      return [];
  }
}

export function codeMirrorTheme(theme: ResolvedTheme): Extension {
  return theme === "dark" ? codeMirrorDarkTheme : codeMirrorLightTheme;
}
