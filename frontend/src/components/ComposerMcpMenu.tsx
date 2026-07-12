/** Composer MCP 快捷菜单：搜索、开关服务、跳转设置。 */
import { useEffect, useMemo, useRef, useState } from "react";
import { PlugZap, Settings2 } from "lucide-react";
import { useMcpTools } from "../hooks/useMcpTools";
import { useI18n } from "../i18n/LocaleContext";

type Props = {
  open: boolean;
  agentId?: string | null;
  onClose: () => void;
  onOpenSettings: () => void;
};

export default function ComposerMcpMenu({
  open,
  agentId,
  onClose,
  onOpenSettings,
}: Props) {
  const { t } = useI18n();
  const { servers, toggleServer } = useMcpTools(agentId);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
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

  useEffect(() => {
    if (!open) return;
    inputRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const onPointer = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) onClose();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onPointer);
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="composer-mcp-menu"
      ref={rootRef}
      role="dialog"
      aria-label={t("chat.mcpMenu")}
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
                    <PlugZap size={14} strokeWidth={2} aria-hidden />
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
    </div>
  );
}
