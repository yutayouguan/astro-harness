/** 聊天消息 Markdown 渲染（用户 / 助手共用）。 */
import {
  useCallback,
  useMemo,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";
import { Check, Copy } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useI18n } from "../i18n/LocaleContext";

/** Markdown 渲染入参 */
type Props = {
  content: string;
  /** 流式输出中（可显示光标等） */
  streaming?: boolean;
  /** 紧凑样式（如工具结果预览） */
  compact?: boolean;
  /** 错误等场景保持纯文本 */
  plain?: boolean;
  /** 是否显示末尾闪烁 caret */
  caret?: boolean;
};

function childrenToText(children: ReactNode): string {
  if (children == null || typeof children === "boolean") return "";
  if (typeof children === "string" || typeof children === "number") {
    return String(children);
  }
  if (Array.isArray(children)) {
    return children.map(childrenToText).join("");
  }
  if (typeof children === "object" && "props" in (children as object)) {
    return childrenToText((children as ReactElement).props?.children);
  }
  return "";
}

function CodeBlock({
  className,
  children,
}: {
  className?: string;
  children?: ReactNode;
}) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const lang = /language-([\w+-]+)/.exec(className ?? "")?.[1] ?? "";
  const codeText = useMemo(
    () => childrenToText(children).replace(/\n$/, ""),
    [children],
  );

  const onCopy = useCallback(async () => {
    if (!codeText) return;
    try {
      await navigator.clipboard.writeText(codeText);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // ignore clipboard failures
    }
  }, [codeText]);

  return (
    <div className="msg-md-codeblock" data-lang={lang || undefined}>
      <div className="msg-md-code-header">
        <span className="msg-md-code-lang">{lang || "code"}</span>
        <button
          type="button"
          className={`msg-md-code-copy ${copied ? "is-copied" : ""}`}
          onClick={() => void onCopy()}
          aria-label={copied ? t("chat.codeCopied") : t("chat.copyCode")}
          title={copied ? t("chat.codeCopied") : t("chat.copyCode")}
        >
          {copied ? (
            <Check size={14} strokeWidth={2.4} aria-hidden />
          ) : (
            <Copy size={14} strokeWidth={2} aria-hidden />
          )}
        </button>
      </div>
      <pre className="msg-md-pre">
        <code className={className}>{children}</code>
      </pre>
    </div>
  );
}

export function ChatMarkdown({
  content,
  streaming = false,
  compact = false,
  plain = false,
  caret = false,
}: Props) {
  const source = useMemo(() => content.replace(/\r\n/g, "\n"), [content]);

  if (plain) {
    return (
      <div
        className={`msg-content ${compact ? "is-compact" : ""} ${
          streaming ? "is-streaming-md" : ""
        }`}
      >
        {source}
        {caret ? <span className="stream-caret" aria-hidden="true" /> : null}
      </div>
    );
  }

  return (
    <div
      className={`msg-content msg-md ${compact ? "is-compact" : ""} ${
        streaming ? "is-streaming-md" : ""
      }`}
    >
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => (
            <a href={href} target="_blank" rel="noreferrer noopener">
              {children}
            </a>
          ),
          code: ({ className, children, ...props }) => {
            const text = String(children ?? "");
            const isBlock =
              Boolean(className?.includes("language-")) || text.includes("\n");
            if (isBlock) {
              return (
                <CodeBlock className={className}>{children}</CodeBlock>
              );
            }
            return (
              <code className="msg-md-inline-code" {...props}>
                {children}
              </code>
            );
          },
          // pre 由 code block 自行包裹，避免双重 pre
          pre: ({ children }) => <>{children}</>,
        }}
      >
        {source}
      </ReactMarkdown>
      {caret ? <span className="stream-caret" aria-hidden="true" /> : null}
    </div>
  );
}
