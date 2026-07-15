/** 近期会话列表。 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Plus } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { RecentSessionDto } from "../types";

/** 近期会话列表入参 */
type Props = {
  /** 当前打开的会话（高亮） */
  activeSessionId: string | null;
  onOpenSession: (sessionId: string) => void;
  /** 新建空白会话 */
  onNewSession: () => void;
};

export default function ChatSessionList({
  activeSessionId,
  onOpenSession,
  onNewSession,
}: Props) {
  const { t } = useI18n();
  const [items, setItems] = useState<RecentSessionDto[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await invoke<RecentSessionDto[]>("list_recent_sessions", { limit: 50 });
      setItems(list ?? []);
      setError(null);
    } catch (e) {
      setError(String(e));
      setItems([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (s) =>
        (s.summary ?? "").toLowerCase().includes(q) ||
        s.sessionId.toLowerCase().includes(q),
    );
  }, [items, query]);

  return (
    <div className="chat-session-list">
      <button
        type="button"
        className="chat-session-new"
        onClick={onNewSession}
      >
        <Plus size={15} strokeWidth={2.2} aria-hidden />
        {t("chat.newSession")}
      </button>
      <input
        className="search-pill"
        type="search"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={t("chat.rightPanel.searchSessions")}
        aria-label={t("chat.rightPanel.searchSessions")}
      />
      {error && <div className="side-error">{error}</div>}
      {filtered.length === 0 ? (
        <p className="muted">{t("chat.rightPanel.noSessions")}</p>
      ) : (
        <ul>
          {filtered.map((s) => (
            <li key={s.sessionId}>
              <button
                type="button"
                className={`chat-session-item ${
                  s.sessionId === activeSessionId ? "is-active" : ""
                }`}
                onClick={() => onOpenSession(s.sessionId)}
              >
                <strong>
                  {(s.summary ?? "").trim() || t("chat.rightPanel.untitledSession")}
                  {s.endReason === "compacted" ? (
                    <span className="chat-session-badge">
                      {t("chat.sessionCompactedBadge")}
                    </span>
                  ) : null}
                </strong>
                <span>{s.sessionId.slice(0, 8)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
