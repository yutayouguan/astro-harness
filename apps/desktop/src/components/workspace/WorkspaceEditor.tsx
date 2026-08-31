/** 工作区文件内容编辑器。 */
import { useMemo } from "react";
import CodeMirror from "@uiw/react-codemirror";
import type { ResolvedTheme } from "../../hooks/app/useTheme";
import {
  codeMirrorTheme,
  languageForFilename,
} from "../../lib/filespace/codeMirrorLanguage";

/** 工作区代码编辑器入参 */
type Props = {
  value: string;
  /** 用于推断语法高亮语言 */
  filename: string;
  theme: ResolvedTheme;
  onChange: (value: string) => void;
};

export default function WorkspaceEditor({
  value,
  filename,
  theme,
  onChange,
}: Props) {
  const extensions = useMemo(
    () => [codeMirrorTheme(theme), ...languageForFilename(filename)],
    [filename, theme],
  );

  return (
    <CodeMirror
      className="ws-codemirror"
      value={value}
      height="100%"
      theme="none"
      extensions={extensions}
      onChange={onChange}
      basicSetup={{
        lineNumbers: true,
        foldGutter: true,
        highlightActiveLine: true,
        highlightActiveLineGutter: true,
        bracketMatching: true,
        autocompletion: false,
        dropCursor: true,
        allowMultipleSelections: true,
        indentOnInput: true,
      }}
    />
  );
}
