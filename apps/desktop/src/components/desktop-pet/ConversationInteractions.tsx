import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { usePendingInteractions } from "../../hooks/chat/usePendingInteractions";
import { inlineInteraction } from "../../lib/chat/pendingInteractions";
import InteractionCard from "./InteractionCard";

export default function ConversationInteractions({
  sessionId,
}: {
  sessionId: string | null | undefined;
}) {
  const { snapshot, connected } = usePendingInteractions();
  const root = useRef<HTMLDivElement>(null);
  const requests = snapshot.requests.filter(
    (r) => r.sessionId === sessionId && inlineInteraction(r),
  );
  const signature = requests.map((r) => r.key).join("|");
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const visible = new Set<Element>();
    const report = () => {
      const keys = [
        ...document.querySelectorAll<HTMLElement>(
          "[data-interaction-visible='true']",
        ),
      ]
        .map((e) => e.dataset.pendingInteractionKey!)
        .filter(Boolean);
      void invoke("report_pending_interactions_visible", { keys }).catch(
        () => {},
      );
    };
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const element = entry.target as HTMLElement;
          element.dataset.interactionVisible = String(
            entry.isIntersecting && entry.intersectionRatio > 0.15,
          );
          visible.add(element);
        }
        report();
      },
      { threshold: [0, 0.15] },
    );
    root.current
      ?.querySelectorAll("[data-pending-interaction-key]")
      .forEach((e) => observer.observe(e));
    window.addEventListener("focus", report);
    window.addEventListener("blur", report);
    return () => {
      observer.disconnect();
      for (const e of visible)
        (e as HTMLElement).dataset.interactionVisible = "false";
      report();
      window.removeEventListener("focus", report);
      window.removeEventListener("blur", report);
    };
  }, [signature]);
  return (
    <div ref={root} className="conversation-interactions" aria-live="polite">
      {requests.map((r) => (
        <InteractionCard key={r.key} request={r} main />
      ))}
      {requests.length > 0 && !connected && <p>正在重新连接…</p>}
    </div>
  );
}
