/** Composer “＋”面板：统一承载附件、Skills 与 MCP 插件。 */
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { FolderOpen, Paperclip, Search, Settings2, Sparkles } from "lucide-react";
import { useClampPopover } from "../../hooks/ui/useClampPopover";
import {
  useMcpTools,
  type McpServer,
} from "../../hooks/providers/useMcpTools";
import { useI18n } from "../../i18n/LocaleContext";
import type { InstalledSkill } from "../../types";
import McpIcon from "../icons/McpIcon";

type Props = {
  open: boolean;
  anchorRef: RefObject<HTMLElement | null>;
  agentId?: string | null;
  skills: InstalledSkill[];
  canAttach: boolean;
  canAttachFolder: boolean;
  onAttach: () => void;
  onAttachFolder: () => void;
  onSelectSkill: (skill: InstalledSkill) => void;
  onSelectMcp: (server: McpServer) => void;
  onClose: () => void;
  onOpenSettings: () => void;
};

export default function ComposerPlusMenu({
  open,
  anchorRef,
  agentId,
  skills,
  canAttach,
  canAttachFolder,
  onAttach,
  onAttachFolder,
  onSelectSkill,
  onSelectMcp,
  onClose,
  onOpenSettings,
}: Props) {
  const { t } = useI18n();
  const reducedMotion = useReducedMotion();
  const { servers, toggleServer } = useMcpTools(agentId);
  const [query, setQuery] = useState("");
  const menuRef = useRef<HTMLDivElement>(null);
  const positionedStyleRef = useRef<CSSProperties>();
  const inputRef = useRef<HTMLInputElement>(null);

  const normalizedQuery = query.trim().toLowerCase();
  const filteredSkills = useMemo(() => {
    if (!normalizedQuery) return skills;
    return skills.filter(
      (skill) =>
        skill.name.toLowerCase().includes(normalizedQuery) ||
        (skill.description?.toLowerCase().includes(normalizedQuery) ?? false),
    );
  }, [normalizedQuery, skills]);
  const filteredServers = useMemo(() => {
    if (!normalizedQuery) return servers;
    return servers.filter(
      (server) =>
        server.name.toLowerCase().includes(normalizedQuery) ||
        server.id.toLowerCase().includes(normalizedQuery),
    );
  }, [normalizedQuery, servers]);

  const style = useClampPopover({
    open,
    anchorRef,
    popoverRef: menuRef,
    mode: "fixed",
    preferAlign: "start",
    placement: "above",
    gap: 8,
    maxHeightCap: 460,
    minMaxHeight: 180,
    sizeKey: `${filteredSkills.length}:${filteredServers.length}:${query}`,
  });
  if (style) positionedStyleRef.current = style;

  useEffect(() => {
    if (!open) {
      setQuery("");
      return;
    }
    window.requestAnimationFrame(() => inputRef.current?.focus());
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    const onPointer = (event: MouseEvent) => {
      const target = event.target as Node;
      if (menuRef.current?.contains(target)) return;
      if (anchorRef.current?.contains(target)) return;
      onClose();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onPointer);
    };
  }, [anchorRef, onClose, open]);

  if (typeof document === "undefined") return null;

  const noPlugins = filteredSkills.length === 0 && filteredServers.length === 0;

  return createPortal(
    <AnimatePresence initial={false}>
      {open ? (
        <motion.div
          key="composer-plus-menu"
          className="composer-mcp-menu composer-plus-menu"
          ref={menuRef}
          role="dialog"
          aria-label={t("chat.plusMenu")}
          style={{
            ...(style ?? positionedStyleRef.current ?? { visibility: "hidden" }),
            transformOrigin: "left bottom",
          }}
          initial={reducedMotion ? { opacity: 0 } : { opacity: 0, y: 6, scale: 0.98 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={
            reducedMotion
              ? { opacity: 0, transition: { duration: 0.1 } }
              : {
                  opacity: 0,
                  y: 4,
                  scale: 0.985,
                  transition: { duration: 0.11, ease: "easeOut" },
                }
          }
          transition={{
            duration: reducedMotion ? 0.1 : 0.16,
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          <div className="composer-plus-section">
            <div className="composer-mcp-menu-group">{t("chat.plusMenuAdd")}</div>
            <button
              type="button"
              className="composer-plus-action"
              disabled={!canAttach}
              onClick={() => {
                onAttach();
                onClose();
              }}
            >
              <Paperclip size={16} strokeWidth={2} aria-hidden />
              <span>
                <strong>{t("chat.plusMenuFiles")}</strong>
                <small>{t("chat.plusMenuFilesHint")}</small>
              </span>
            </button>
            <button
              type="button"
              className="composer-plus-action"
              disabled={!canAttachFolder}
              onClick={() => {
                onAttachFolder();
                onClose();
              }}
            >
              <FolderOpen size={16} strokeWidth={2} aria-hidden />
              <span>
                <strong>{t("chat.plusMenuFolder")}</strong>
                <small>{t("chat.plusMenuFolderHint")}</small>
              </span>
            </button>
          </div>

          <div className="composer-mcp-menu-search composer-plus-search">
            <Search size={14} strokeWidth={2} aria-hidden />
            <input
              ref={inputRef}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("chat.plusMenuSearch")}
              aria-label={t("chat.plusMenuSearch")}
            />
          </div>

          <div className="composer-mcp-menu-body">
            <div className="composer-mcp-menu-group">{t("chat.plusMenuPlugins")}</div>
            {noPlugins ? (
              <p className="composer-mcp-menu-empty">{t("chat.plusMenuEmpty")}</p>
            ) : (
              <>
                {filteredSkills.length > 0 ? (
                  <div className="composer-plus-subgroup">
                    <div className="composer-plus-subgroup-title">
                      <Sparkles size={13} strokeWidth={2} aria-hidden />
                      Skills
                    </div>
                    <ul className="composer-mcp-menu-list">
                      {filteredSkills.map((skill) => (
                        <li key={skill.id}>
                          <button
                            type="button"
                            className="composer-plus-plugin-button"
                            onClick={() => {
                              onSelectSkill(skill);
                              onClose();
                            }}
                          >
                            <span className="composer-mcp-menu-name" title={skill.name}>
                              {skill.name}
                            </span>
                            <span className="composer-plus-plugin-hint">
                              {t("chat.plusMenuUseSkill")}
                            </span>
                          </button>
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : null}

                {filteredServers.length > 0 ? (
                  <div className="composer-plus-subgroup">
                    <div className="composer-plus-subgroup-title">
                      <McpIcon size={13} />
                      MCP
                    </div>
                    <ul className="composer-mcp-menu-list">
                      {filteredServers.map((server) => (
                        <li key={server.id} className="composer-mcp-menu-row">
                          <button
                            type="button"
                            className="composer-plus-plugin-button composer-plus-plugin-select"
                            onClick={() => {
                              onSelectMcp(server);
                              onClose();
                            }}
                          >
                            <span className="composer-mcp-menu-name" title={server.name}>
                              {server.name}
                            </span>
                            <span className="composer-plus-plugin-hint">
                              {t("chat.plusMenuUseMcp")}
                            </span>
                          </button>
                          <button
                            type="button"
                            role="switch"
                            className="tool-toggle"
                            aria-checked={server.enabled}
                            aria-label={server.name}
                            onClick={() => toggleServer(server.id)}
                          >
                            <span className="tool-toggle-thumb" />
                          </button>
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : null}
              </>
            )}
          </div>

          <button
            type="button"
            className="composer-mcp-menu-footer"
            onClick={() => {
              onOpenSettings();
              onClose();
            }}
          >
            <Settings2 size={14} strokeWidth={2} aria-hidden />
            {t("chat.mcpMenuOpenSettings")}
          </button>
        </motion.div>
      ) : null}
    </AnimatePresence>,
    document.body,
  );
}
