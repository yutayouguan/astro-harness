import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { Bot, File, FileVideo, Image, Music2, Sparkles, X } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import type { ComposerContextToken } from "../../lib/chat/composerContext";
import type { ChatAttachment, SkillContent } from "../../types";
import { useI18n } from "../../i18n/LocaleContext";
import McpIcon from "../icons/McpIcon";
import { ChatMarkdown } from "./ChatMarkdown";

export type ComposerPreviewTarget =
  | { type: "attachment"; item: ChatAttachment }
  | { type: "context"; item: ComposerContextToken };

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function isTextAttachment(item: ChatAttachment): boolean {
  return (
    item.mime.startsWith("text/") ||
    /\.(txt|md|json|csv|xml|yaml|yml|toml|rs|ts|tsx|js|py|html|css)$/i.test(
      item.name,
    )
  );
}

function decodeText(base64: string | undefined): string | null {
  if (!base64) return null;
  try {
    const bytes = Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}

function ContextIcon({ token }: { token: ComposerContextToken }) {
  if (token.kind === "skill") return <Sparkles size={18} aria-hidden />;
  if (token.kind === "mcp") return <McpIcon size={18} />;
  return <Bot size={18} aria-hidden />;
}

function AttachmentIcon({ item }: { item: ChatAttachment }) {
  if (item.kind === "image") return <Image size={18} aria-hidden />;
  if (item.kind === "video") return <FileVideo size={18} aria-hidden />;
  if (item.kind === "audio") return <Music2 size={18} aria-hidden />;
  return <File size={18} aria-hidden />;
}

export default function ComposerContextPreview({
  target,
  onClose,
}: {
  target: ComposerPreviewTarget | null;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const reducedMotion = useReducedMotion();
  const closeRef = useRef<HTMLButtonElement>(null);
  const [skillContent, setSkillContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!target) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    window.requestAnimationFrame(() => closeRef.current?.focus());
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose, target]);

  useEffect(() => {
    let cancelled = false;
    setSkillContent(null);
    if (target?.type !== "context" || target.item.kind !== "skill") {
      setLoading(false);
      return () => {
        cancelled = true;
      };
    }
    setLoading(true);
    void invoke<SkillContent>("get_skill_content", { name: target.item.name })
      .then((result) => {
        if (!cancelled) setSkillContent(result.content || null);
      })
      .catch(() => {
        if (!cancelled) setSkillContent(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [target]);

  if (typeof document === "undefined") return null;

  const title = target?.item.name ?? "";
  const contextKindLabel =
    target?.type === "context"
      ? {
          agent: t("chat.contextToken.agent"),
          skill: t("chat.contextToken.skill"),
          mcp: t("chat.contextToken.mcp"),
        }[target.item.kind]
      : null;
  const attachmentText =
    target?.type === "attachment" && isTextAttachment(target.item)
      ? decodeText(target.item.dataBase64)
      : null;

  return createPortal(
    <AnimatePresence initial={false}>
      {target ? (
        <motion.div
          key="composer-context-preview"
          className="composer-context-preview-backdrop"
          role="presentation"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0, transition: { duration: 0.12, ease: "easeOut" } }}
          transition={{ duration: reducedMotion ? 0.1 : 0.14, ease: "easeOut" }}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) onClose();
          }}
        >
          <motion.section
            className="composer-context-preview-dialog"
            role="dialog"
            aria-modal="true"
            aria-label={t("chat.contextPreviewTitle", { name: title })}
            initial={reducedMotion ? { opacity: 0 } : { opacity: 0, y: 8, scale: 0.985 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={
              reducedMotion
                ? { opacity: 0, transition: { duration: 0.1 } }
                : {
                    opacity: 0,
                    y: 4,
                    scale: 0.99,
                    transition: { duration: 0.12, ease: "easeOut" },
                  }
            }
            transition={{
              duration: reducedMotion ? 0.1 : 0.18,
              ease: [0.22, 1, 0.36, 1],
            }}
          >
            <header className="composer-context-preview-header">
              <span className="composer-context-preview-icon">
                {target.type === "attachment" ? (
                  <AttachmentIcon item={target.item} />
                ) : (
                  <ContextIcon token={target.item} />
                )}
              </span>
              <span className="composer-context-preview-heading">
                <strong>{title}</strong>
                <small>
                  {target.type === "attachment"
                    ? `${target.item.mime} · ${formatSize(target.item.size)}`
                    : contextKindLabel}
                </small>
              </span>
              <button
                ref={closeRef}
                type="button"
                className="composer-context-preview-close"
                onClick={onClose}
                aria-label={t("chat.contextPreviewClose")}
              >
                <X size={17} aria-hidden />
              </button>
            </header>
            <div className="composer-context-preview-body">
              {target.type === "attachment" && target.item.kind === "image" && target.item.previewUrl ? (
                <img src={target.item.previewUrl} alt={target.item.name} />
              ) : target.type === "attachment" && target.item.kind === "video" && target.item.previewUrl ? (
                <video src={target.item.previewUrl} controls />
              ) : target.type === "attachment" && target.item.kind === "audio" && target.item.previewUrl ? (
                <audio src={target.item.previewUrl} controls />
              ) : attachmentText ? (
                <pre>{attachmentText}</pre>
              ) : target.type === "context" && target.item.kind === "skill" && skillContent ? (
                <ChatMarkdown content={skillContent} />
              ) : (
                <div className="composer-context-preview-summary">
                  {loading ? (
                    <span>{t("chat.contextPreviewLoading")}</span>
                  ) : (
                    <>
                      <p>
                        {target.type === "context"
                          ? target.item.description || t("chat.contextPreviewUnavailable")
                          : t("chat.contextPreviewUnavailable")}
                      </p>
                      {target.type === "attachment" && target.item.localPath ? (
                        <code>{target.item.localPath}</code>
                      ) : target.type === "context" ? (
                        <code>{target.item.path || target.item.id}</code>
                      ) : null}
                    </>
                  )}
                </div>
              )}
            </div>
          </motion.section>
        </motion.div>
      ) : null}
    </AnimatePresence>,
    document.body,
  );
}
