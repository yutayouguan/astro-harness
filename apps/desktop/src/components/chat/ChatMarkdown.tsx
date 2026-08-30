/** 聊天消息 Markdown 渲染（用户 / 助手共用）；本地媒体路径与 HTML 代码块可预览。 */
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactElement,
  type ReactNode,
} from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { useI18n } from "../../i18n/LocaleContext";
import { liftHtmlMediaTags } from "../../lib/chat/liftHtmlMediaTags";
import {
  absolutizeMediaPath,
  resolveMediaSrc,
  stripFileUrl,
} from "../../lib/media/resolveMediaSrc";
import {
  isCodePath,
  type GeneratedMediaKind,
} from "../../lib/media/parseGeneratedMedia";
import BrokenMedia from "../media/BrokenMedia";
import GeneratedMediaCard from "../media/GeneratedMediaCard";
import HtmlPreview from "../media/HtmlPreview";
import { CopyMorphIcon } from "../icons/MorphIcon";

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
  /** Agent 工作区根；Markdown 相对路径（如 generated/x.png）据此解析 */
  mediaBaseDir?: string | null;
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

function mediaKindFromMarkdown(
  src: string | undefined,
  alt: string | undefined,
): GeneratedMediaKind {
  const hint = (alt ?? "").trim().toLowerCase();
  if (
    hint === "audio" ||
    hint === "video" ||
    hint === "image" ||
    hint === "html" ||
    hint === "code"
  ) {
    return hint;
  }
  const path = (src ?? "").split("?")[0]?.toLowerCase() ?? "";
  if (/\.(mp3|wav|m4a|aac|ogg|flac|opus|wma)$/i.test(path)) return "audio";
  if (/\.(mp4|webm|mov|mkv|m4v|avi)$/i.test(path)) return "video";
  if (/\.(html?)$/i.test(path)) return "html";
  if (isCodePath(path)) return "code";
  return "image";
}

function MarkdownMedia({
  src,
  alt,
  baseDir,
}: {
  src?: string;
  alt?: string;
  baseDir?: string | null;
}) {
  const kind = mediaKindFromMarkdown(src, alt);
  const pathForActions = useMemo(() => {
    const abs = absolutizeMediaPath(src, baseDir);
    if (abs) return abs;
    return src?.trim() ?? "";
  }, [src, baseDir]);

  const resolved = useMemo(
    () => resolveMediaSrc(src, baseDir),
    [src, baseDir],
  );

  if (!resolved || !pathForActions) {
    return <BrokenMedia path={src} />;
  }

  return (
    <GeneratedMediaCard
      kind={kind}
      path={pathForActions}
      compact
      className="msg-md-media-card"
    />
  );
}

function CodeCopyButton({
  codeText,
  style,
}: {
  codeText: string;
  style?: CSSProperties;
}) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const resetTimerRef = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (resetTimerRef.current !== null) {
        window.clearTimeout(resetTimerRef.current);
      }
    },
    [],
  );

  const onCopy = useCallback(async () => {
    if (!codeText) return;
    try {
      await navigator.clipboard.writeText(codeText);
      setCopied(true);
      if (resetTimerRef.current !== null) {
        window.clearTimeout(resetTimerRef.current);
      }
      resetTimerRef.current = window.setTimeout(() => {
        setCopied(false);
        resetTimerRef.current = null;
      }, 1200);
    } catch {
      // Clipboard availability depends on the active webview permissions.
    }
  }, [codeText]);

  const label = copied ? t("chat.codeCopied") : t("chat.copyCode");

  return (
    <button
      type="button"
      className={`msg-md-code-copy ${copied ? "is-copied" : ""}`}
      onClick={() => void onCopy()}
      aria-label={label}
      title={label}
      style={style}
    >
      <span className="msg-md-code-copy-feedback" role="status" aria-live="polite">
        {copied ? t("chat.codeCopied") : ""}
      </span>
      <CopyMorphIcon copied={copied} size={15} aria-hidden />
    </button>
  );
}

function HtmlCodeBlock({
  className,
  children,
}: {
  className?: string;
  children?: ReactNode;
}) {
  const { t } = useI18n();
  const [mode, setMode] = useState<"source" | "preview">("source");
  const lang = /language-([\w+-]+)/.exec(className ?? "")?.[1] ?? "";
  const codeText = useMemo(
    () => childrenToText(children).replace(/\n$/, ""),
    [children],
  );

  return (
    <div className="msg-md-html-block" data-lang={lang || undefined}>
      <div className="msg-md-html-toggle" role="tablist">
        <button
          type="button"
          role="tab"
          className={mode === "source" ? "is-active" : ""}
          aria-selected={mode === "source"}
          onClick={() => setMode("source")}
        >
          {t("media.htmlSource")}
        </button>
        <button
          type="button"
          role="tab"
          className={mode === "preview" ? "is-active" : ""}
          aria-selected={mode === "preview"}
          onClick={() => setMode("preview")}
        >
          {t("media.htmlShowPreview")}
        </button>
        <CodeCopyButton codeText={codeText} style={{ marginLeft: "auto" }} />
      </div>
      {mode === "preview" ? (
        <HtmlPreview source={codeText} compact />
      ) : (
        <div
          className="msg-md-codeblock is-html-source"
          data-lang={lang || undefined}
          data-single-line={!codeText.includes("\n") || undefined}
        >
          <pre className="msg-md-pre">
            <code className={className}>{children}</code>
          </pre>
        </div>
      )}
    </div>
  );
}

function CodeBlock({
  className,
  children,
}: {
  className?: string;
  children?: ReactNode;
}) {
  const lang = /language-([\w+-]+)/.exec(className ?? "")?.[1] ?? "";
  const codeText = useMemo(
    () => childrenToText(children).replace(/\n$/, ""),
    [children],
  );

  return (
    <div
      className="msg-md-codeblock"
      data-lang={lang || undefined}
      data-single-line={!codeText.includes("\n") || undefined}
    >
      <div className="msg-md-code-header">
        <span className="msg-md-code-lang">{lang || "code"}</span>
        <CodeCopyButton codeText={codeText} />
      </div>
      <pre className="msg-md-pre">
        <code className={className}>{children}</code>
      </pre>
    </div>
  );
}

function LocalHtmlLink({
  href,
  children,
}: {
  href?: string;
  children?: ReactNode;
}) {
  const { t } = useI18n();
  const [showPreview, setShowPreview] = useState(false);
  const path = href?.trim() ?? "";
  const isLocalHtml =
    /\.(html?)$/i.test(path) &&
    (path.startsWith("/") ||
      /^[A-Za-z]:[\\/]/.test(path) ||
      /^file:/i.test(path));

  if (!isLocalHtml) {
    return (
      <a href={href} target="_blank" rel="noreferrer noopener">
        {children}
      </a>
    );
  }

  return (
    <span className="msg-md-html-link">
      <a href={href} target="_blank" rel="noreferrer noopener">
        {children}
      </a>{" "}
      <button
        type="button"
        className="msg-md-inline-preview-btn"
        onClick={() => setShowPreview((v) => !v)}
      >
        {showPreview ? t("media.htmlSource") : t("media.htmlShowPreview")}
      </button>
      {showPreview ? <HtmlPreview path={stripFileUrl(path)} compact /> : null}
    </span>
  );
}

function ChatMarkdownImpl({
  content,
  streaming = false,
  compact = false,
  plain = false,
  caret = false,
  mediaBaseDir = null,
}: Props) {
  const source = useMemo(
    () => liftHtmlMediaTags(content.replace(/\r\n/g, "\n")),
    [content],
  );

  // 组件映射必须保持引用稳定：react-markdown 以函数引用作为组件类型，
  // 每次新建会导致 <img> 等节点整棵卸载重挂（图片重载 → 闪烁）。
  const components = useMemo<Components>(
    () => ({
      a: ({ href, children }) => (
        <LocalHtmlLink href={href}>{children}</LocalHtmlLink>
      ),
      img: ({ src, alt }) => (
        <MarkdownMedia src={src} alt={alt} baseDir={mediaBaseDir} />
      ),
      table: ({ children }) => (
        <div className="msg-md-table-wrap">
          <table>{children}</table>
        </div>
      ),
      code: ({ className, children, ...props }) => {
        const text = String(children ?? "");
        const isBlock =
          Boolean(className?.includes("language-")) || text.includes("\n");
        if (isBlock) {
          const lang =
            /language-([\w+-]+)/.exec(className ?? "")?.[1]?.toLowerCase() ?? "";
          if (lang === "html" || lang === "htm") {
            return (
              <HtmlCodeBlock className={className}>{children}</HtmlCodeBlock>
            );
          }
          return <CodeBlock className={className}>{children}</CodeBlock>;
        }
        return (
          <code className="msg-md-inline-code" {...props}>
            {children}
          </code>
        );
      },
      // pre 由 code block 自行包裹，避免双重 pre
      pre: ({ children }) => <>{children}</>,
    }),
    [mediaBaseDir],
  );

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
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {source}
      </ReactMarkdown>
      {caret ? <span className="stream-caret" aria-hidden="true" /> : null}
    </div>
  );
}

export const ChatMarkdown = memo(ChatMarkdownImpl);
