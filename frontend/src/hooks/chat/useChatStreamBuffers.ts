import { useCallback, useRef } from "react";
import type { Dispatch, SetStateAction } from "react";
import {
  applyActivityUpsert,
  applyReasoningDelta,
  sealOpenReasoning,
} from "../../lib/chat/chatTimeline";
import { elapsedSecSince } from "../../lib/chat/elapsedSec";
import type { ChatActivity, ChatMessage, MessageTokenUsage } from "../../types";

function calcTokensPerSec(
  completionTokens: number,
  durationMs: number,
): number | undefined {
  if (completionTokens <= 0 || durationMs <= 0) return undefined;
  const sec = Math.max(0.1, durationMs / 1000);
  return Math.round((completionTokens / sec) * 10) / 10;
}

export function useChatStreamBuffers(
  setMessages: Dispatch<SetStateAction<ChatMessage[]>>,
) {
  const streamPendingRef = useRef<Map<string, string>>(new Map());
  const streamReasoningPendingRef = useRef<Map<string, string>>(new Map());
  const streamStartRef = useRef<Map<string, number>>(new Map());
  const firstTokenRef = useRef<Map<string, number>>(new Map());
  const pendingUsageRef = useRef<Map<string, MessageTokenUsage>>(new Map());
  const activeAssistantIdRef = useRef<string | null>(null);
  const streamRafRef = useRef<number | null>(null);
  const toolDeltaPendingRef = useRef<
    Map<
      string,
      { messageId: string; index: number; id: string; name: string; args: string }
    >
  >(new Map());
  const toolDeltaRafRef = useRef<number | null>(null);
  const toolDeltaIdsRef = useRef<Map<string, string>>(new Map());
  const streamGenRef = useRef(0);
  const currentRunIdRef = useRef<string | null>(null);

  const flushStreamTokens = useCallback(() => {
    streamRafRef.current = null;
    const batch = new Map(streamPendingRef.current);
    const reasoningBatch = new Map(streamReasoningPendingRef.current);
    streamPendingRef.current.clear();
    streamReasoningPendingRef.current.clear();
    if (batch.size === 0 && reasoningBatch.size === 0) return;
    const now = Date.now();
    setMessages((prev) =>
      prev.map((m) => {
        const extra = batch.get(m.id);
        const reasoningExtra = reasoningBatch.get(m.id);
        if (!extra && !reasoningExtra) return m;
        let next = m;
        if (reasoningExtra) {
          next = applyReasoningDelta(next, reasoningExtra, now);
        }
        // first content token: seal any open reasoning segment
        if (extra && !next.content && next.reasoning) {
          next = sealOpenReasoning(next, now);
        }
        next = {
          ...next,
          content: extra ? next.content + extra : next.content,
        };
        return next;
      }),
    );
  }, [setMessages]);

  const flushToolDeltas = useCallback(() => {
    toolDeltaRafRef.current = null;
    const batch = Array.from(toolDeltaPendingRef.current.values());
    toolDeltaPendingRef.current.clear();
    if (batch.length === 0) return;
    setMessages((prev) =>
      prev.map((m) => {
        const mine = batch.filter((d) => d.messageId === m.id);
        if (mine.length === 0) return m;
        let next = m;
        for (const d of mine) {
          const mapKey = `${m.id}:${d.index}`;
          let actId = toolDeltaIdsRef.current.get(mapKey);
          const activities = next.activities ?? [];
          let idx = actId
            ? activities.findIndex((a) => a.id === actId)
            : -1;
          if (idx < 0 && d.id) {
            idx = activities.findIndex((a) => a.id === d.id);
          }
          let activity: ChatActivity;
          if (idx < 0) {
            actId = d.id || `tc-${d.index}-${Date.now()}`;
            toolDeltaIdsRef.current.set(mapKey, actId);
            activity = {
              id: actId,
              kind: "tool",
              title: d.name || `tool#${d.index}`,
              input: d.args || undefined,
              detail: d.args || undefined,
              status: "running",
              at: Date.now(),
            };
          } else {
            const cur = activities[idx]!;
            if (d.id) toolDeltaIdsRef.current.set(mapKey, d.id);
            const argsSoFar =
              cur.status === "running" ? (cur.input ?? cur.detail ?? "") : "";
            const nextArgs = d.args ? argsSoFar + d.args : cur.input ?? cur.detail;
            activity = {
              ...cur,
              id: d.id || cur.id,
              title: d.name || cur.title,
              input: nextArgs || undefined,
              detail: nextArgs || undefined,
              status: "running",
            };
          }
          next = applyActivityUpsert(next, activity);
        }
        return next;
      }),
    );
  }, [setMessages]);

  const enqueueStreamToken = useCallback(
    (messageId: string, token: string) => {
      if (!token) return;
      if (!firstTokenRef.current.has(messageId)) {
        firstTokenRef.current.set(messageId, Date.now());
      }
      streamPendingRef.current.set(
        messageId,
        (streamPendingRef.current.get(messageId) ?? "") + token,
      );
      if (streamRafRef.current == null) {
        streamRafRef.current = requestAnimationFrame(flushStreamTokens);
      }
    },
    [flushStreamTokens],
  );

  const enqueueStreamReasoning = useCallback(
    (messageId: string, token: string) => {
      if (!token) return;
      streamReasoningPendingRef.current.set(
        messageId,
        (streamReasoningPendingRef.current.get(messageId) ?? "") + token,
      );
      if (streamRafRef.current == null) {
        streamRafRef.current = requestAnimationFrame(flushStreamTokens);
      }
    },
    [flushStreamTokens],
  );

  const enqueueToolDelta = useCallback(
    (
      messageId: string,
      delta: { index: number; id?: string; name?: string; arguments?: string },
    ) => {
      const key = `${messageId}:${delta.index}`;
      const prev = toolDeltaPendingRef.current.get(key);
      toolDeltaPendingRef.current.set(key, {
        messageId,
        index: delta.index,
        id: (delta.id?.trim() || prev?.id || "").trim(),
        name: (delta.name?.trim() || prev?.name || "").trim(),
        args: (prev?.args ?? "") + (delta.arguments ?? ""),
      });
      if (toolDeltaRafRef.current == null) {
        toolDeltaRafRef.current = requestAnimationFrame(flushToolDeltas);
      }
    },
    [flushToolDeltas],
  );

  const clearStreamBuffers = useCallback(() => {
    if (streamRafRef.current != null) {
      cancelAnimationFrame(streamRafRef.current);
      streamRafRef.current = null;
    }
    if (toolDeltaRafRef.current != null) {
      cancelAnimationFrame(toolDeltaRafRef.current);
      toolDeltaRafRef.current = null;
    }
    streamPendingRef.current.clear();
    streamReasoningPendingRef.current.clear();
    streamStartRef.current.clear();
    firstTokenRef.current.clear();
    pendingUsageRef.current.clear();
    activeAssistantIdRef.current = null;
    toolDeltaPendingRef.current.clear();
    toolDeltaIdsRef.current.clear();
  }, []);

  const settleMessageUsage = useCallback(
    (messageId: string, endedAt = Date.now()) => {
      const usage = pendingUsageRef.current.get(messageId);
      const start =
        firstTokenRef.current.get(messageId) ??
        streamStartRef.current.get(messageId);
      const tokensPerSec =
        usage && start != null
          ? calcTokensPerSec(usage.completionTokens, endedAt - start)
          : undefined;
      streamStartRef.current.delete(messageId);
      firstTokenRef.current.delete(messageId);
      pendingUsageRef.current.delete(messageId);
      setMessages((prev) =>
        prev.map((m) => {
          if (m.id !== messageId) return m;
          const generationDurationSec =
            m.generationDurationSec ??
            (m.generationStartedAt != null
              ? elapsedSecSince(m.generationStartedAt, endedAt)
              : undefined);
          if (!usage && tokensPerSec == null && generationDurationSec == null) {
            return m;
          }
          return {
            ...m,
            usage: usage ?? m.usage,
            tokensPerSec: tokensPerSec ?? m.tokensPerSec,
            generationDurationSec:
              generationDurationSec ?? m.generationDurationSec,
            generationStartedAt: undefined,
          };
        }),
      );
    },
    [setMessages],
  );

  return {
    // enqueuers
    enqueueStreamToken,
    enqueueStreamReasoning,
    enqueueToolDelta,
    // flushers
    flushStreamTokens,
    flushToolDeltas,
    // control
    clearStreamBuffers,
    settleMessageUsage,
    // refs exposed for send / stop
    streamGenRef,
    currentRunIdRef,
    activeAssistantIdRef,
    streamStartRef,
    firstTokenRef,
    pendingUsageRef,
    streamPendingRef,
    toolDeltaIdsRef,
    toolDeltaRafRef,
    streamRafRef,
  };
}
