import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type UseSideChatSessionArgs = {
  hostSessionId: string | null;
  messageCount: number;
  disabled: boolean;
  onBeforeOpen: () => void;
  onError: (message: string) => void;
};

export function canStartSideChat({
  hostSessionId,
  messageCount,
  disabled,
  currentSessionId,
}: Pick<
  UseSideChatSessionArgs,
  "hostSessionId" | "messageCount" | "disabled"
> & { currentSessionId: string | null }): boolean {
  return Boolean(
    hostSessionId && !disabled && !currentSessionId && messageCount > 0,
  );
}

/** Owns the ephemeral side-chat lifecycle independently from App shell layout. */
export function useSideChatSession({
  hostSessionId,
  messageCount,
  disabled,
  onBeforeOpen,
  onError,
}: UseSideChatSessionArgs) {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [parentSessionId, setParentSessionId] = useState<string | null>(null);

  const close = useCallback(async () => {
    const sideId = sessionId;
    setSessionId(null);
    setParentSessionId(null);
    if (!sideId) return;
    await invoke("discard_side_session", { sessionId: sideId }).catch(
      (error) => {
        console.warn("discard_side_session failed", error);
      },
    );
  }, [sessionId]);

  const start = useCallback(async () => {
    if (
      !canStartSideChat({
        hostSessionId,
        messageCount,
        disabled,
        currentSessionId: sessionId,
      })
    )
      return;
    try {
      const id = await invoke<string>("fork_chat_session", {
        sourceSessionId: hostSessionId,
        keepChatBubbles: messageCount,
        sourceMessageId: null,
        boundary: "through_turn",
        ephemeral: true,
        excludeTurns: true,
        newSessionId: null,
      });
      onBeforeOpen();
      setSessionId(id);
      setParentSessionId(hostSessionId);
    } catch (error) {
      onError(String(error));
    }
  }, [disabled, hostSessionId, messageCount, onBeforeOpen, onError, sessionId]);

  const openExisting = useCallback(
    (nextSessionId: string, nextParentSessionId: string | null) => {
      onBeforeOpen();
      setSessionId(nextSessionId);
      setParentSessionId(nextParentSessionId);
    },
    [onBeforeOpen],
  );

  useEffect(() => {
    if (sessionId && parentSessionId && hostSessionId !== parentSessionId) {
      void close();
    }
  }, [close, hostSessionId, parentSessionId, sessionId]);

  return {
    sessionId,
    parentSessionId,
    open: Boolean(sessionId),
    start,
    openExisting,
    close,
  };
}
