/** 技能包文件只读预览：CodeMirror 高亮、MD 预览/源码、复制。 */
import { useCallback, useMemo, useState } from "react";
import CodeMirror from "@uiw/react-codemirror";
import {
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  Eye,
  FileCode2,
  FolderOpen,
  Tags,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { useTheme } from "../../hooks/useTheme";
import {
  codeMirrorTheme,
  languageForFilename,
} from "../../lib/filespace/codeMirrorLanguage";
import {
  splitSkillFrontmatter,
  type SkillFrontmatter,
} from "../../lib/skills/skillFrontmatter";
import { ChatMarkdown } from "../chat/ChatMarkdown";

const MD_MODE_KEY = "astro.skills.mdPreviewMode";
export const SKILL_PREVIEW_MAX_BYTES = 512 * 1024;

type MdMode = "preview" | "source";

function readMdMode(): MdMode {
  try {
    const v = localStorage.getItem(MD_MODE_KEY);
    if (v === "source" || v === "preview") return v;
  } catch {
    // ignore
  }
  return "preview";
}

function isMarkdownPath(filename: string): boolean {
  const lower = filename.toLowerCase();
  return lower.endsWith(".md") || lower.endsWith(".markdown");
}

function normText(s: string | undefined | null): string {
  return (s ?? "").trim().replace(/\s+/g, " ");
}

function SkillFrontmatterCard({
  meta,
  knownName,
  knownDescription,
}: {
  meta: SkillFrontmatter;
  /** 抽屉顶栏已展示的名称；相同则不再重复 */
  knownName?: string;
  /** 抽屉顶栏已展示的描述；相同则不再重复 */
  knownDescription?: string;
}) {
  const { t } = useI18n();
  const [descOpen, setDescOpen] = useState(false);
  const [extrasOpen, setExtrasOpen] = useState(false);

  const showName =
    Boolean(meta.name?.trim()) &&
    normText(meta.name) !== normText(knownName);
  const desc = meta.description?.trim() ?? "";
  const showDesc =
    Boolean(desc) && normText(desc) !== normText(knownDescription);
  const descLong = desc.length > 160 || desc.includes("\n");
  const showExtras = meta.extras.length > 0;

  if (!showName && !showDesc && !showExtras) return null;

  return (
    <section className="skills-frontmatter" aria-label={t("skills.frontmatterTitle")}>
      <header className="skills-frontmatter-head">
        <Tags size={13} strokeWidth={2.3} aria-hidden />
        <span>{t("skills.frontmatterTitle")}</span>
      </header>
      {showName || showDesc ? (
        <dl className="skills-frontmatter-fields">
          {showName ? (
            <div className="skills-frontmatter-row">
              <dt>{t("skills.frontmatterName")}</dt>
              <dd>
                <code className="skills-frontmatter-name">{meta.name}</code>
              </dd>
            </div>
          ) : null}
          {showDesc ? (
            <div className="skills-frontmatter-row">
              <dt>{t("skills.frontmatterDescription")}</dt>
              <dd>
                <p
                  className={`skills-frontmatter-desc ${descOpen || !descLong ? "is-open" : ""}`}
                >
                  {desc}
                </p>
                {descLong ? (
                  <button
                    type="button"
                    className="skills-frontmatter-toggle"
                    onClick={() => setDescOpen((v) => !v)}
                  >
                    {descOpen ? (
                      <ChevronUp size={13} strokeWidth={2.3} aria-hidden />
                    ) : (
                      <ChevronDown size={13} strokeWidth={2.3} aria-hidden />
                    )}
                    {descOpen
                      ? t("skills.frontmatterCollapse")
                      : t("skills.frontmatterExpand")}
                  </button>
                ) : null}
              </dd>
            </div>
          ) : null}
        </dl>
      ) : null}
      {showExtras ? (
        <div className="skills-frontmatter-extras">
          <button
            type="button"
            className="skills-frontmatter-toggle"
            onClick={() => setExtrasOpen((v) => !v)}
            aria-expanded={extrasOpen}
          >
            {extrasOpen ? (
              <ChevronUp size={13} strokeWidth={2.3} aria-hidden />
            ) : (
              <ChevronDown size={13} strokeWidth={2.3} aria-hidden />
            )}
            {t("skills.frontmatterMore")}
            <span className="skills-frontmatter-extra-count">
              {meta.extras.length}
            </span>
          </button>
          {extrasOpen ? (
            <dl className="skills-frontmatter-extra-list">
              {meta.extras.map((item) => (
                <div key={item.key} className="skills-frontmatter-row">
                  <dt>{item.key}</dt>
                  <dd>{item.value}</dd>
                </div>
              ))}
            </dl>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

type Props = {
  content: string | null;
  filename: string;
  /** 工具栏展示的路径（与预览/源码同一行） */
  pathLabel?: string;
  /** 工具栏展示的体积文案 */
  sizeLabel?: string;
  /** 文件字节大小；超限时不展示编辑器 */
  size?: number;
  loading?: boolean;
  onOpenExternal?: () => void;
  /** 在访达中显示当前文件 */
  onReveal?: () => void;
  /** 抽屉顶栏已有的技能名，用于去重视觉重复 */
  skillName?: string;
  /** 抽屉顶栏已有的技能描述，用于去重视觉重复 */
  skillDescription?: string;
};

export function SkillFileViewer({
  content,
  filename,
  pathLabel,
  sizeLabel,
  size = 0,
  loading = false,
  onOpenExternal,
  onReveal,
  skillName,
  skillDescription,
}: Props) {
  const { t } = useI18n();
  const { resolved } = useTheme();
  const [mdMode, setMdMode] = useState<MdMode>(() => readMdMode());
  const [copied, setCopied] = useState(false);
  const tooLarge = size > SKILL_PREVIEW_MAX_BYTES;
  const isMd = isMarkdownPath(filename);
  const showPreview = isMd && mdMode === "preview" && !tooLarge && content != null;

  const split = useMemo(
    () => (content != null ? splitSkillFrontmatter(content) : null),
    [content],
  );

  const extensions = useMemo(
    () => [codeMirrorTheme(resolved), ...languageForFilename(filename)],
    [filename, resolved],
  );

  const setMode = (mode: MdMode) => {
    setMdMode(mode);
    try {
      localStorage.setItem(MD_MODE_KEY, mode);
    } catch {
      // ignore
    }
  };

  const onCopy = useCallback(async () => {
    if (!content) return;
    try {
      await navigator.clipboard.writeText(content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // ignore
    }
  }, [content]);

  if (loading) {
    return (
      <p className="skills-preview-empty">{t("skills.previewLoading")}</p>
    );
  }

  if (tooLarge) {
    return (
      <div className="skills-preview-binary">
        <p>{t("skills.previewTooLarge")}</p>
        {onOpenExternal ? (
          <button
            type="button"
            className="skills-action-btn"
            onClick={onOpenExternal}
          >
            {t("skills.previewOpenExternal")}
          </button>
        ) : null}
      </div>
    );
  }

  if (content == null) {
    return (
      <p className="skills-preview-empty">{t("skills.previewLoading")}</p>
    );
  }

  return (
    <div className="skills-file-viewer">
      <div className="skills-file-viewer-toolbar">
        {(pathLabel || sizeLabel) && (
          <div className="skills-file-viewer-meta" title={pathLabel}>
            {pathLabel ? (
              <span className="skills-file-viewer-path">{pathLabel}</span>
            ) : null}
            {sizeLabel ? (
              <span className="skills-file-viewer-size">{sizeLabel}</span>
            ) : null}
          </div>
        )}
        {isMd ? (
          <div className="skills-file-viewer-modes" role="tablist">
            <button
              type="button"
              role="tab"
              aria-selected={mdMode === "preview"}
              className={`skills-file-viewer-mode ${mdMode === "preview" ? "is-active" : ""}`}
              onClick={() => setMode("preview")}
            >
              <Eye size={13} strokeWidth={2.3} aria-hidden />
              {t("skills.previewMode")}
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={mdMode === "source"}
              className={`skills-file-viewer-mode ${mdMode === "source" ? "is-active" : ""}`}
              onClick={() => setMode("source")}
            >
              <FileCode2 size={13} strokeWidth={2.3} aria-hidden />
              {t("skills.previewSource")}
            </button>
          </div>
        ) : !pathLabel ? (
          <span className="skills-file-viewer-label">{filename}</span>
        ) : null}
        <div className="skills-file-viewer-actions">
          {onReveal ? (
            <button
              type="button"
              className="skills-file-viewer-reveal"
              onClick={onReveal}
              title={t("skills.revealFile")}
              aria-label={t("skills.revealFile")}
            >
              <FolderOpen size={14} strokeWidth={2.3} aria-hidden />
            </button>
          ) : null}
          <button
            type="button"
            className={`skills-file-viewer-copy ${copied ? "is-copied" : ""}`}
            onClick={() => void onCopy()}
            title={copied ? t("skills.copied") : t("skills.copyContent")}
            aria-label={copied ? t("skills.copied") : t("skills.copyContent")}
          >
            {copied ? (
              <Check size={14} strokeWidth={2.4} aria-hidden />
            ) : (
              <Copy size={14} strokeWidth={2.2} aria-hidden />
            )}
            <span>{copied ? t("skills.copied") : t("skills.copyContent")}</span>
          </button>
        </div>
      </div>
      <div className="skills-file-viewer-body">
        {showPreview ? (
          <div className="skills-file-viewer-md">
            {split?.frontmatter ? (
              <SkillFrontmatterCard
                meta={split.frontmatter}
                knownName={skillName}
                knownDescription={skillDescription}
              />
            ) : null}
            {split?.body.trim() ? (
              <ChatMarkdown content={split.body} />
            ) : !split?.frontmatter ? (
              <ChatMarkdown content={content} />
            ) : null}
          </div>
        ) : (
          <CodeMirror
            className="skills-file-codemirror"
            value={content}
            height="100%"
            theme="none"
            editable={false}
            extensions={extensions}
            basicSetup={{
              lineNumbers: true,
              foldGutter: true,
              highlightActiveLine: false,
              highlightActiveLineGutter: false,
              bracketMatching: true,
              autocompletion: false,
              dropCursor: false,
              allowMultipleSelections: false,
              indentOnInput: false,
            }}
          />
        )}
      </div>
    </div>
  );
}
