/**
 * MultiTask：每条消息独立 session 并行流式，不占用主会话 unlisten / sessionId。
 */
import { useCallback, useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { applyActivityUpsert, applySurfaceUpsert, sealOpenReasoning } from "../../lib/chat/chatTimeline";
import {
  countRunningParallel,
  isParallelTaskActive,
  MAX_PARALLEL_RUNNING,
  newParallelTaskId,
  type ParallelChatTask,
} from "../../lib/chat/parallelTasks";
import {
  loadPickerGlobals,
  loadModelPrefs,
  modelPrefsToApi,
} from "../../lib/model/modelPrefs";
import { shouldShowThinkingControls } from "../../lib/chat/shouldShowThinkingControls";
import type {
  ChatActivity,
  ChatAttachment,
  ChatMessage,
  PendingInterrupt,
  ProviderDto,
  UiSurface,
} from "../../types";
import type { MessageKey } from "../../i18n/messages";
import type { ShowToastOptions } from "../ui/useTransientToast";

type TFn = (key: MessageKey, vars?: Record<string, string>) => string;
type ShowToastFn = (msg: string, opts?: ShowToastOptions) => void;

export type StartParallelTaskOpts = {
  text: string;
  attachments?: ChatAttachment[];
};

type Deps = {
  activeProvider: ProviderDto | undefined;
  setMessages: Dispatch<SetStateAction<ChatMessage[]>>;
  setEmptyMode: Dispatch<SetStateAction<"chat" | "agent" | null>>;
  setInput: Dispatch<SetStateAction<string>>;
  setAttachments: Dispatch<SetStateAction<ChatAttachment[]>>;
  showTransientToast: ShowToastFn;
  t: TFn;
};

export function useParallelTasks(deps: Deps) {
  const depsRef = useRef(deps);
  depsRef.current = deps;

  const [parallelTasks, setParallelTasks] = useState<ParallelChatTask[]>([]);
  const parallelTasksRef = useRef(parallelTasks);
  parallelTasksRef.current = parallelTasks;
  const unlistenMapRef = useRef<Map<string, { unlisten: UnlistenFn; sessionId: string }>>(
    new Map(),
  );
  const streamBufRef = useRef<Map<string, string>>(new Map());
  const rafMapRef = useRef<Map<string, number>>(new Map());
  /** 已占槽但尚未写入 running task 的并发启动数（防 TOCTOU） */
  const pendingStartsRef = useRef(0);

  const flushToken = useCallback((assistantId: string) => {
    const pending = streamBufRef.current.get(assistantId) ?? "";
    if (!pending) return;
    streamBufRef.current.set(assistantId, "");
    depsRef.current.setMessages((prev) =>
      prev.map((m) =>
        m.id === assistantId ? { ...m, content: (m.content || "") + pending } : m,
      ),
    );
  }, []);

  const enqueueToken = useCallback(
    (assistantId: string, token: string) => {
      streamBufRef.current.set(
        assistantId,
        (streamBufRef.current.get(assistantId) ?? "") + token,
      );
      if (rafMapRef.current.has(assistantId)) return;
      const raf = requestAnimationFrame(() => {
        rafMapRef.current.delete(assistantId);
        flushToken(assistantId);
      });
      rafMapRef.current.set(assistantId, raf);
    },
    [flushToken],
  );

  const cleanupTaskStream = useCallback((taskId: string, assistantId: string) => {
    const raf = rafMapRef.current.get(assistantId);
    if (raf != null) {
      cancelAnimationFrame(raf);
      rafMapRef.current.delete(assistantId);
    }
    flushToken(assistantId);
    streamBufRef.current.delete(assistantId);
    const entry = unlistenMapRef.current.get(taskId);
    if (entry) {
      entry.unlisten();
      unlistenMapRef.current.delete(taskId);
    }
  }, [flushToken]);

  const clearAllParallel = useCallback(() => {
    setParallelTasks((prev) => {
      for (const task of prev) {
        if (task.worktree) {
          void invoke("cleanup_multitask_worktree", {
            path: task.worktree.path,
            repoRoot: task.worktree.repoRoot,
            branch: task.worktree.branch,
          }).catch(() => {});
        }
      }
      return [];
    });
    for (const [taskId, entry] of unlistenMapRef.current) {
      entry.unlisten();
      void invoke("chat_control", {
        sessionId: entry.sessionId,
        action: "cancel",
      }).catch(() => {});
      void taskId;
    }
    unlistenMapRef.current.clear();
    for (const raf of rafMapRef.current.values()) cancelAnimationFrame(raf);
    rafMapRef.current.clear();
    streamBufRef.current.clear();
  }, []);

  // 卸载时清理
  useEffect(() => {
    return () => {
      for (const entry of unlistenMapRef.current.values()) entry.unlisten();
      unlistenMapRef.current.clear();
      for (const raf of rafMapRef.current.values()) cancelAnimationFrame(raf);
      rafMapRef.current.clear();
    };
  }, []);

  const cancelParallelTask = useCallback(
    async (taskId: string) => {
      const task = parallelTasksRef.current.find((t) => t.id === taskId);
      if (!task || !isParallelTaskActive(task.status)) return;
      cleanupTaskStream(taskId, task.assistantMessageId);
      try {
        await invoke("chat_control", { sessionId: task.sessionId, action: "cancel" });
      } catch (e) {
        console.warn("parallel task cancel failed", e);
      }
      setParallelTasks((prev) =>
        prev.map((t) =>
          t.id === taskId
            ? {
                ...t,
                status: "cancelled",
                finishedAt: Date.now(),
                pendingInterrupts: undefined,
              }
            : t,
        ),
      );
      if (task.worktree) {
        void invoke("cleanup_multitask_worktree", {
          path: task.worktree.path,
          repoRoot: task.worktree.repoRoot,
          branch: task.worktree.branch,
        }).catch(() => {});
      }
      depsRef.current.setMessages((prev) =>
        prev.map((m) => {
          if (m.id !== task.assistantMessageId) return m;
          return sealOpenReasoning(
            {
              ...m,
              content: (m.content || "").trim() || depsRef.current.t("chat.task.cancelled"),
              generationStartedAt: undefined,
              uiSurfaces: m.uiSurfaces?.map((s) => ({
                ...s,
                status: "cancelled" as const,
              })),
            },
            Date.now(),
          );
        }),
      );
    },
    [cleanupTaskStream],
  );

  /** 并行气泡 HITL 批准/澄清：按 task.sessionId resume，不碰主会话 streaming */
  const resumeParallelHitl = useCallback(
    async (
      assistantMessageId: string,
      payload: Record<string, unknown>,
    ): Promise<boolean> => {
      const task = parallelTasksRef.current.find(
        (t) =>
          t.assistantMessageId === assistantMessageId &&
          t.status === "waiting" &&
          (t.pendingInterrupts?.length ?? 0) > 0,
      );
      if (!task?.pendingInterrupts?.length) return false;

      const resumeJson = JSON.stringify(
        task.pendingInterrupts.map((p) => ({
          interrupt_id: p.id,
          status: "resolved",
          payload,
        })),
      );

      depsRef.current.setMessages((prev) =>
        prev.map((m) => {
          if (m.id !== assistantMessageId) return m;
          return {
            ...m,
            uiSurfaces: m.uiSurfaces?.map((s) => ({
              ...s,
              status: "resolved" as const,
            })),
          };
        }),
      );
      setParallelTasks((prev) =>
        prev.map((t) =>
          t.id === task.id
            ? { ...t, status: "running", pendingInterrupts: undefined }
            : t,
        ),
      );

      try {
        await invoke("interrupt_resume", {
          sessionId: task.sessionId,
          resumeJson,
        });
        return true;
      } catch (e) {
        depsRef.current.showTransientToast(
          e instanceof Error ? e.message : String(e ?? "HITL resume failed"),
          { tone: "error" },
        );
        setParallelTasks((prev) =>
          prev.map((t) =>
            t.id === task.id
              ? {
                  ...t,
                  status: "waiting",
                  pendingInterrupts: task.pendingInterrupts,
                }
              : t,
          ),
        );
        return true; // 已路由到并行，勿再走主会话
      }
    },
    [],
  );

  const startParallelTask = useCallback(async (opts: StartParallelTaskOpts) => {
    const {
      activeProvider,
      setMessages,
      setEmptyMode,
      setInput,
      setAttachments,
      showTransientToast,
      t,
    } = depsRef.current;

    const text = opts.text.trim();
    const pending = opts.attachments ?? [];
    if (!text && pending.length === 0) return false;
    if (!activeProvider) {
      showTransientToast(t("status.none"), { tone: "warning" });
      return false;
    }

    let blocked = false;
    setParallelTasks((prev) => {
      if (countRunningParallel(prev) + pendingStartsRef.current >= MAX_PARALLEL_RUNNING) {
        blocked = true;
        return prev;
      }
      pendingStartsRef.current += 1;
      return prev;
    });
    if (blocked) {
      showTransientToast(
        t("chat.task.limit", { max: String(MAX_PARALLEL_RUNNING) }),
        { tone: "warning" },
      );
      return false;
    }

    const releasePendingSlot = () => {
      pendingStartsRef.current = Math.max(0, pendingStartsRef.current - 1);
    };

    const taskId = newParallelTaskId();
    const sessionId = crypto.randomUUID();
    const userId = `u-${taskId}`;
    const assistantId = `a-${taskId}`;
    const createdAt = Date.now();

    let worktree: ParallelChatTask["worktree"];
    let slotted = true;
    try {
      const prepared = await invoke<{
        path: string;
        repoRoot: string;
        branch: string;
      } | null>("prepare_multitask_worktree", { taskId });
      if (prepared?.path) {
        worktree = {
          path: prepared.path,
          repoRoot: prepared.repoRoot,
          branch: prepared.branch,
        };
      }
    } catch (e) {
      console.warn("prepare_multitask_worktree failed", e);
    }

    const task: ParallelChatTask = {
      id: taskId,
      sessionId,
      prompt: text,
      userMessageId: userId,
      assistantMessageId: assistantId,
      status: "running",
      createdAt,
      worktree,
    };

    try {
      setParallelTasks((prev) => [task, ...prev]);
      releasePendingSlot();
      slotted = false;
    } finally {
      if (slotted) releasePendingSlot();
    }

    setMessages((prev) => [
      ...prev,
      {
        id: userId,
        role: "user",
        content: text,
        attachments: pending.map((a) => ({ ...a })),
        createdAt,
      },
      {
        id: assistantId,
        role: "assistant",
        content: "",
        activities: [],
        createdAt,
        generationStartedAt: createdAt,
      },
    ]);
    setEmptyMode(null);
    setInput("");
    setAttachments([]);

    // 交互模式说明由后端写入 system prompt，不拼进用户消息
    const contentForModel = text;
    const globals = loadPickerGlobals();
    const sendSupportsThinking = shouldShowThinkingControls({
      capabilities: null,
      backendId: activeProvider.backend_id,
    });
    const modelApi = sendSupportsThinking
      ? modelPrefsToApi(
          loadModelPrefs(activeProvider.id, activeProvider.model),
          globals,
        )
      : { thinkingEnabled: false, reasoningEffort: "high" as const };

    const finish = (status: ParallelChatTask["status"], error?: string) => {
      cleanupTaskStream(taskId, assistantId);
      setParallelTasks((prev) =>
        prev.map((t) =>
          t.id === taskId
            ? {
                ...t,
                status,
                error,
                finishedAt: Date.now(),
                pendingInterrupts: undefined,
              }
            : t,
        ),
      );
    };

    try {
      const eventName = `chat-stream-${sessionId}`;
      const unlisten = await listen<{
        type: string;
        content?: string;
        message?: string;
        id?: string;
        name?: string;
        arguments?: string;
        result?: string;
        index?: number;
        outcome_type?: string;
        interrupts_json?: string;
        message_id?: string;
        activity_type?: string;
        content_json?: string;
        media?: Array<{
          kind?: string;
          ref_value?: string;
        }>;
      }>(eventName, (event) => {
        const payload = event.payload;
        if (payload.type === "token" && payload.content) {
          enqueueToken(assistantId, payload.content);
        } else if (payload.type === "reasoning" && payload.content) {
          setMessages((prev) =>
            prev.map((m) =>
              m.id === assistantId
                ? { ...m, reasoning: (m.reasoning || "") + payload.content }
                : m,
            ),
          );
        } else if (payload.type === "activity") {
          let operations: unknown[] = [];
          try {
            const parsed = JSON.parse(payload.content_json || "{}") as {
              operations?: unknown;
            };
            if (Array.isArray(parsed.operations)) {
              operations = parsed.operations;
            }
          } catch {
            /* ignore malformed activity */
          }
          const surface: UiSurface = {
            messageId: payload.message_id || `surf-${Date.now()}`,
            activityType: payload.activity_type || "a2ui-surface",
            operations,
            status: "active",
          };
          setMessages((prev) =>
            prev.map((m) => {
              if (m.id !== assistantId) return m;
              return applySurfaceUpsert(m, surface);
            }),
          );
        } else if (
          payload.type === "run_finished" &&
          (payload.outcome_type === "interrupt" ||
            payload.outcome_type === "hitl_waiting")
        ) {
          let interrupts: PendingInterrupt[] = [];
          try {
            const arr = JSON.parse(payload.interrupts_json || "[]") as unknown;
            if (Array.isArray(arr)) {
              interrupts = arr
                .map((raw) => {
                  const i = raw as Record<string, unknown>;
                  let responseSchema: unknown;
                  const schemaRaw = i.response_schema_json;
                  if (typeof schemaRaw === "string" && schemaRaw.trim()) {
                    try {
                      responseSchema = JSON.parse(schemaRaw);
                    } catch {
                      responseSchema = undefined;
                    }
                  }
                  return {
                    id: String(i.id ?? ""),
                    reason: String(i.reason ?? ""),
                    message: typeof i.message === "string" ? i.message : undefined,
                    responseSchema,
                    assistantMessageId: assistantId,
                  } satisfies PendingInterrupt;
                })
                .filter((i) => i.id);
            }
          } catch {
            interrupts = [];
          }
          setParallelTasks((prev) =>
            prev.map((t) =>
              t.id === taskId
                ? { ...t, status: "waiting", pendingInterrupts: interrupts }
                : t,
            ),
          );
          setMessages((prev) =>
            prev.map((m) => {
              if (m.id !== assistantId) return m;
              let next = m;
              const surfaces = [...(m.uiSurfaces ?? [])];
              if (surfaces.length > 0) {
                const last = surfaces[surfaces.length - 1]!;
                next = applySurfaceUpsert(next, {
                  ...last,
                  interrupts: interrupts.map(
                    ({ id, reason, message, responseSchema }) => ({
                      id,
                      reason,
                      message,
                      responseSchema,
                    }),
                  ),
                });
              }
              return sealOpenReasoning(next, Date.now());
            }),
          );
        } else if (payload.type === "run_finished" && payload.outcome_type === "success") {
          setParallelTasks((prev) =>
            prev.map((t) =>
              t.id === taskId ? { ...t, pendingInterrupts: undefined } : t,
            ),
          );
        } else if (payload.type === "tool_call") {
          const name = payload.name ?? "tool";
          const activity: ChatActivity = {
            id: payload.id || `act-${Date.now()}`,
            kind: "tool",
            title: name,
            input: payload.arguments,
            output: payload.result,
            status: payload.result ? "done" : "running",
            at: Date.now(),
          };
          setMessages((prev) =>
            prev.map((m) =>
              m.id === assistantId ? applyActivityUpsert(m, activity) : m,
            ),
          );
        } else if (payload.type === "done") {
          const raf = rafMapRef.current.get(assistantId);
          if (raf != null) {
            cancelAnimationFrame(raf);
            rafMapRef.current.delete(assistantId);
          }
          flushToken(assistantId);
          setMessages((prev) =>
            prev.map((m) => {
              if (m.id !== assistantId) return m;
              const pendingText = streamBufRef.current.get(assistantId) ?? "";
              const content = (m.content + pendingText).trim();
              streamBufRef.current.delete(assistantId);
              const sealed = sealOpenReasoning(
                {
                  ...m,
                  generationStartedAt: undefined,
                  generationDurationSec:
                    m.generationDurationSec ??
                    (m.generationStartedAt != null
                      ? Math.max(0, (Date.now() - m.generationStartedAt) / 1000)
                      : undefined),
                },
                Date.now(),
              );
              if (
                !content &&
                !(m.activities && m.activities.length > 0) &&
                !(m.uiSurfaces && m.uiSurfaces.length > 0)
              ) {
                return {
                  ...sealed,
                  content: t("status.emptyResponse"),
                  error: true,
                };
              }
              return sealed;
            }),
          );
          finish("done");
        } else if (payload.type === "error") {
          const errMsg = payload.message || t("status.unknownError");
          flushToken(assistantId);
          setMessages((prev) =>
            prev.map((m) => {
              if (m.id !== assistantId) return m;
              const base = (m.content ?? "").trim();
              return sealOpenReasoning(
                {
                  ...m,
                  content: base ? `${base}\n\n⚠️ ${errMsg}` : errMsg,
                  error: true,
                  generationStartedAt: undefined,
                },
                Date.now(),
              );
            }),
          );
          finish("error", errMsg);
        }
      });
      unlistenMapRef.current.set(taskId, { unlisten, sessionId });

      await invoke<string>("start_chat", {
        content: contentForModel,
        provider: activeProvider.backend_id,
        providerId: activeProvider.id,
        model: activeProvider.model,
        sessionId,
        useMemory: true,
        thinkingEnabled: modelApi.thinkingEnabled,
        reasoningEffort: modelApi.reasoningEffort,
        interactionMode: "multitask",
        projectRoot: worktree?.path,
        attachments: pending.map((a) => ({
          name: a.name,
          mime: a.mime,
          kind: a.kind,
          size: a.size,
          dataBase64: a.dataBase64 ?? null,
          localPath: a.localPath ?? null,
        })),
      });
      return true;
    } catch (err) {
      const errMsg = String(err);
      finish("error", errMsg);
      if (worktree) {
        void invoke("cleanup_multitask_worktree", {
          path: worktree.path,
          repoRoot: worktree.repoRoot,
          branch: worktree.branch,
        }).catch(() => {});
      }
      setMessages((prev) =>
        prev.map((m) =>
          m.id === assistantId
            ? { ...m, content: errMsg, error: true, generationStartedAt: undefined }
            : m,
        ),
      );
      showTransientToast(errMsg, { tone: "error" });
      return false;
    }
  }, [cleanupTaskStream, enqueueToken, flushToken]);

  const clearSettledParallel = useCallback(() => {
    setParallelTasks((prev) => {
      for (const task of prev) {
        if (isParallelTaskActive(task.status)) continue;
        if (task.worktree) {
          void invoke("cleanup_multitask_worktree", {
            path: task.worktree.path,
            repoRoot: task.worktree.repoRoot,
            branch: task.worktree.branch,
          }).catch(() => {});
        }
      }
      return prev.filter((t) => isParallelTaskActive(t.status));
    });
  }, []);

  const parallelRunning = countRunningParallel(parallelTasks);

  return {
    parallelTasks,
    parallelRunning,
    startParallelTask,
    cancelParallelTask,
    resumeParallelHitl,
    clearAllParallel,
    clearSettledParallel,
    setParallelTasks,
  };
}
