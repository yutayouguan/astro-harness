/** Composer MCP 快捷菜单：搜索、开关服务、跳转设置（portal + 启发式定位）。 */
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { Settings2 } from "lucide-react";
import { useClampPopover } from "../../hooks/useClampPopover";
import { useMcpTools } from "../../hooks/useMcpTools";
import { useI18n } from "../../i18n/LocaleContext";
import McpIcon from "../icons/McpIcon";

type Props = {
  open: boolean;
  /** 触发按钮外层（composer-mcp-wrap） */
  anchorRef: RefObject<HTMLElement | null>;
  agentId?: string | null;
  onClose: () => void;
  onOpenSettings: () => void;
};

export default function ComposerMcpMenu({
  open,
  anchorRef,
  agentId,
  onClose,
  onOpenSettings,
}: Props) {
  const { t } = useI18n();
  const { servers, toggleServer } = useMcpTools(agentId);
  const [query, setQuery] = useState("");
  const menuRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return servers;
    return servers.filter(
      (s) =>
        s.name.toLowerCase().includes(q) ||
        s.id.toLowerCase().includes(q),
    );
  }, [servers, query]);

  const style = useClampPopover({
    open,
    anchorRef,
    popoverRef: menuRef,
    mode: "fixed",
    preferAlign: "start",
    placement: "above",
    gap: 8,
    maxHeightCap: 420,
    minMaxHeight: 120,
    sizeKey: `${filtered.length}:${query}`,
  });

  useEffect(() => {
    if (!open) {
      setQuery("");
      return;
    }
    inputRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const onPointer = (e: MouseEvent) => {
      const target = e.target as Node;
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
  }, [open, onClose, anchorRef]);

  if (!open || typeof document === "undefined") return null;

  return createPortal(
    <div
      className="composer-mcp-menu"
      ref={menuRef}
      role="dialog"
      aria-label={t("chat.mcpMenu")}
      style={style ?? { visibility: "hidden" }}
    >
      <div className="composer-mcp-menu-search">
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("chat.mcpMenuSearch")}
          aria-label={t("chat.mcpMenuSearch")}
        />
      </div>
      <div className="composer-mcp-menu-body">
        {filtered.length === 0 ? (
          <p className="composer-mcp-menu-empty">{t("chat.mcpMenuEmpty")}</p>
        ) : (
          <>
            <div className="composer-mcp-menu-group">{t("chat.mcpMenuUserGroup")}</div>
            <ul className="composer-mcp-menu-list">
              {filtered.map((s) => (
                <li key={s.id} className="composer-mcp-menu-row">
                  <span className="composer-mcp-menu-name" title={s.name}>
                    <McpIcon size={14} />
                    {s.name}
                  </span>
                  <button
                    type="button"
                    role="switch"
                    className="tool-toggle"
                    aria-checked={s.enabled}
                    aria-label={s.name}
                    onClick={() => toggleServer(s.id)}
                  >
                    <span className="tool-toggle-thumb" />
                  </button>
                </li>
              ))}
            </ul>
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
    </div>,
    document.body,
  );
}
