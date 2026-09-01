/**
 * 用户显式创建的独立任务：每条消息使用独立 session 并行流式。
 */
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  applyActivityUpsert,
  applyReasoningDelta,
  applySurfaceUpsert,
  applyTextDelta,
  reconcileReasoning,
  reconcileText,
  sealOpenReasoning,
} from "../../lib/chat/chatTimeline";
import { consumeBufferedTextReconcile } from "../../lib/chat/streamReconcile";
import { upsertAsyncAgentUpdate } from "../../lib/chat/asyncAgentUpdate";
import { resolveParallelTaskCompletion } from "../../lib/chat/taskCompletion";
import {
  isLiveActivityStatus,
  isSettledActivityStatus,
  resolveToolActivityStatus,
} from "../../lib/chat/toolActivityStatus";
import { parseHitlRunFinished } from "../../lib/chat/hitlRunFinished";
import {
  buildElicitationContent,
  elicitationRequestId,
  resolveElicitationAction,
} from "../../lib/chat/elicitation";
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
import { dispatchSessionsChanged } from "../../lib/chat/sessionManagement";
import type {
  ChatActivity,
  ChatAttachment,
  ConversationEntry,
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
  /** 直接从输入框发送时清空 composer；队列转移到新任务时保持当前草稿。 */
  clearComposer?: boolean;
};

type Deps = {
  activeProvider: ProviderDto | undefined;
  setMessages: Dispatch<SetStateAction<ConversationEntry[]>>;
  setEmptyMode: Dispatch<SetStateAction<"chat" | "agent" | null>>;
  setInput: Dispatch<SetStateAction<string>>;
  setAttachments: Dispatch<SetStateAction<ChatAttachment[]>>;
  showTransientToast: ShowToastFn;
  t: TFn;
  /** 独立任务成功完成；失败、取消或等待审批均不触发。 */
  onTaskSucceeded?: () => void;
};

export function useParallelTasks(deps: Deps) {
  const depsRef = useRef(deps);
  depsRef.current = deps;

  const [parallelTasks, setParallelTasks] = useState<ParallelChatTask[]>([]);
  const parallelTasksRef = useRef(parallelTasks);
  parallelTasksRef.current = parallelTasks;
  const unlistenMapRef = useRef<
    Map<string, { unlisten: UnlistenFn; sessionId: string }>
  >(new Map());
  const streamBufRef = useRef<Map<string, string>>(new Map());
  const rafMapRef = useRef<Map<string, number>>(new Map());
  /** 已占槽但尚未写入 running task 的并发启动数（防 TOCTOU） */
  const pendingStartsRef = useRef(0);

  const flushToken = useCallback((assistantId: string) => {
    const pending = streamBufRef.current.get(assistantId) ?? "";
    if (!pending) return;
    streamBufRef.current.set(assistantId, "");
    depsRef.current.setMessages((prev) =>
      prev.map((m) => (m.id === assistantId ? applyTextDelta(m, pending) : m)),
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

  const cleanupTaskStream = useCallback(
    (taskId: string, assistantId: string) => {
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
    },
    [flushToken],
  );

  const clearAllParallel = useCallback(() => {
    setParallelTasks((prev) => {
      for (const task of prev) {
        if (task.worktree) {
          void invoke("cleanup_task_worktree", {
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
        await invoke("chat_control", {
          sessionId: task.sessionId,
          action: "cancel",
        });
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
      dispatchSessionsChanged();
      if (task.worktree) {
        void invoke("cleanup_task_worktree", {
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
              content:
                (m.content || "").trim() ||
                depsRef.current.t("chat.task.cancelled"),
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
      actionName: string,
    ): Promise<boolean> => {
      const task = parallelTasksRef.current.find(
        (t) =>
          t.assistantMessageId === assistantMessageId &&
          t.status === "waiting" &&
          (t.pendingInterrupts?.length ?? 0) > 0,
      );
      if (!task?.pendingInterrupts?.length) return false;

      const elicitation = task.pendingInterrupts.find(
        (item) => item.reason === "elicitation",
      );
      if (elicitation) {
        const action = resolveElicitationAction(actionName);
        const metadataPayload = elicitation.metadata?.payload;
        const metadata =
          metadataPayload &&
          typeof metadataPayload === "object" &&
          !Array.isArray(metadataPayload)
            ? (metadataPayload as Record<string, unknown>)
            : undefined;
        const serverName =
          typeof metadata?.server_name === "string" ? metadata.server_name : "";
        if (!serverName) {
          depsRef.current.showTransientToast(
            "MCP elicitation routing metadata is missing",
            { tone: "error" },
          );
          return true;
        }
        try {
          await invoke("resolve_elicitation", {
            sessionId: task.sessionId,
            serverName,
            requestId: elicitationRequestId(elicitation),
            action,
            contentJson:
              action !== "accept"
                ? null
                : JSON.stringify(buildElicitationContent(elicitation, payload)),
            metaJson: null,
          });
          setParallelTasks((prev) =>
            prev.map((entry) => {
              if (entry.id !== task.id) return entry;
              const remaining = entry.pendingInterrupts?.filter(
                (interrupt) => interrupt.id !== elicitation.id,
              );
              return {
                ...entry,
                status: remaining?.length ? "waiting" : "running",
                pendingInterrupts: remaining?.length ? remaining : undefined,
              };
            }),
          );
        } catch (error) {
          depsRef.current.showTransientToast(
            error instanceof Error ? error.message : String(error),
            { tone: "error" },
          );
        }
        return true;
      }

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

  const startParallelTask = useCallback(
    async (opts: StartParallelTaskOpts) => {
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
        if (
          countRunningParallel(prev) + pendingStartsRef.current >=
          MAX_PARALLEL_RUNNING
        ) {
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
        } | null>("prepare_task_worktree", { taskId });
        if (prepared?.path) {
          worktree = {
            path: prepared.path,
            repoRoot: prepared.repoRoot,
            branch: prepared.branch,
          };
        }
      } catch (e) {
        console.warn("prepare_task_worktree failed", e);
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
          turnStatus: "running",
          createdAt,
          generationStartedAt: createdAt,
        },
      ]);
      setEmptyMode(null);
      if (opts.clearComposer !== false) {
        setInput("");
        setAttachments([]);
      }

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

      const finish = (
        status: ParallelChatTask["status"],
        error?: string,
        celebrate = false,
      ) => {
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
        if (celebrate) depsRef.current.onTaskSucceeded?.();
      };

      try {
        const eventName = `chat_stream_${sessionId}`;
        let terminalError: string | undefined;
        let terminalOutcome: string | null = null;
        let hasTextOutput = false;
        let hasStructuredOutput = false;
        let completionSettled = false;
        const unlisten = await listen<{
          type: string;
          content?: string;
          message?: string;
          id?: string;
          name?: string;
          arguments?: string;
          arguments_json?: string;
          result?: string;
          delta?: string;
          phase?: string;
          batch_id?: string;
          execution_mode?: "serial" | "parallel";
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
            if (payload.content.trim()) hasTextOutput = true;
            enqueueToken(assistantId, payload.content);
          } else if (
            payload.type === "async_message" &&
            payload.id &&
            payload.content
          ) {
            setMessages((prev) =>
              upsertAsyncAgentUpdate(
                prev,
                assistantId,
                payload.id!,
                payload.content!,
              ),
            );
          } else if (payload.type === "text_reconcile") {
            const raf = rafMapRef.current.get(assistantId);
            if (raf != null) {
              cancelAnimationFrame(raf);
              rafMapRef.current.delete(assistantId);
            }
            const buffered = streamBufRef.current.get(assistantId) ?? "";
            flushToken(assistantId);
            const reconciled = consumeBufferedTextReconcile(
              "",
              buffered,
              payload.content ?? "",
            );
            hasTextOutput = reconciled.content.trim().length > 0;
            streamBufRef.current.set(assistantId, reconciled.buffered);
            setMessages((prev) =>
              prev.map((message) =>
                message.id === assistantId
                  ? reconcileText(message, reconciled.content)
                  : message,
              ),
            );
          } else if (payload.type === "reasoning" && payload.content) {
            flushToken(assistantId);
            setMessages((prev) =>
              prev.map((m) =>
                m.id === assistantId
                  ? applyReasoningDelta(m, payload.content!)
                  : m,
              ),
            );
          } else if (payload.type === "reasoning_reconcile") {
            flushToken(assistantId);
            setMessages((prev) =>
              prev.map((message) =>
                message.id === assistantId
                  ? reconcileReasoning(message, payload.content ?? "")
                  : message,
              ),
            );
          } else if (payload.type === "activity") {
            hasStructuredOutput = true;
            flushToken(assistantId);
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
            terminalOutcome = payload.outcome_type ?? null;
            const { interrupts } = parseHitlRunFinished(
              payload.interrupts_json,
              assistantId,
            );
            const waitingToolIds = new Set(
              interrupts
                .map((interrupt) => interrupt.toolCallId)
                .filter((id): id is string => Boolean(id)),
            );
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
                let next: ConversationEntry = {
                  ...m,
                  turnStatus:
                    payload.outcome_type === "interrupt"
                      ? ("interrupted" as const)
                      : ("waiting" as const),
                  activities: m.activities?.map((activity) => {
                    if (
                      payload.outcome_type === "hitl_waiting" &&
                      waitingToolIds.has(activity.id)
                    ) {
                      return { ...activity, status: "waiting" as const };
                    }
                    if (
                      payload.outcome_type === "interrupt" &&
                      isLiveActivityStatus(activity.status)
                    ) {
                      return { ...activity, status: "interrupted" as const };
                    }
                    return activity;
                  }),
                };
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
          } else if (
            payload.type === "run_finished" &&
            payload.outcome_type === "success"
          ) {
            terminalOutcome = payload.outcome_type;
            setParallelTasks((prev) =>
              prev.map((t) =>
                t.id === taskId ? { ...t, pendingInterrupts: undefined } : t,
              ),
            );
          } else if (
            payload.type === "run_finished" &&
            payload.outcome_type === "error"
          ) {
            terminalOutcome = payload.outcome_type;
            terminalError ??= t("status.unknownError");
          } else if (payload.type === "run_finished") {
            terminalOutcome = payload.outcome_type ?? null;
          } else if (payload.type === "tool_call") {
            hasStructuredOutput = true;
            flushToken(assistantId);
            const name = payload.name ?? "tool";
            const activity: ChatActivity = {
              id: payload.id || `act-${Date.now()}`,
              kind: "tool",
              title: name,
              input: payload.arguments_json ?? payload.arguments,
              output: payload.result,
              status: resolveToolActivityStatus(payload.phase, payload.result),
              at: Date.now(),
              batchId: payload.batch_id,
              executionMode: payload.execution_mode,
            };
            setMessages((prev) =>
              prev.map((m) =>
                m.id === assistantId ? applyActivityUpsert(m, activity) : m,
              ),
            );
          } else if (
            payload.type === "tool_output_delta" &&
            payload.id &&
            payload.delta
          ) {
            const { id, delta } = payload as { id: string; delta: string };
            setMessages((prev) =>
              prev.map((m) => {
                if (m.id !== assistantId) return m;
                const existing = (m.activities ?? []).find((a) => a.id === id);
                if (!existing || isSettledActivityStatus(existing.status))
                  return m;
                const output = `${existing.output ?? ""}${delta}`;
                return applyActivityUpsert(m, {
                  ...existing,
                  output,
                  status: "running",
                });
              }),
            );
          } else if (payload.type === "done") {
            const completion = resolveParallelTaskCompletion({
              outcome: terminalOutcome,
              terminalError,
              hasRenderableOutput: hasTextOutput || hasStructuredOutput,
              emptyResponseError: t("status.emptyResponse"),
            });
            if (completion.status == null) return;
            if (completionSettled) return;
            completionSettled = true;
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
                    turnStatus:
                      completion.status === "cancelled"
                        ? ("interrupted" as const)
                        : completion.status === "error"
                          ? ("error" as const)
                          : ("done" as const),
                    generationStartedAt: undefined,
                    generationDurationSec:
                      m.generationDurationSec ??
                      (m.generationStartedAt != null
                        ? Math.max(
                            0,
                            (Date.now() - m.generationStartedAt) / 1000,
                          )
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
                    content:
                      completion.status === "cancelled"
                        ? t("chat.task.cancelled")
                        : (completion.error ?? t("status.emptyResponse")),
                    error: completion.status === "error",
                  };
                }
                return sealed;
              }),
            );
            finish(
              completion.status,
              completion.error ?? undefined,
              completion.celebrate,
            );
          } else if (payload.type === "error") {
            const errMsg = payload.message || t("status.unknownError");
            terminalError = errMsg;
            flushToken(assistantId);
            setMessages((prev) =>
              prev.map((m) => {
                if (m.id !== assistantId) return m;
                const base = (m.content ?? "").trim();
                return sealOpenReasoning(
                  {
                    ...m,
                    turnStatus: "error" as const,
                    content: base ? `${base}\n\n⚠️ ${errMsg}` : errMsg,
                    error: true,
                    generationStartedAt: undefined,
                  },
                  Date.now(),
                );
              }),
            );
          }
        });
        unlistenMapRef.current.set(taskId, { unlisten, sessionId });

        await invoke<string>("start_chat", {
          request: {
            content: contentForModel,
            provider: activeProvider.backend_id,
            providerId: activeProvider.id,
            model: activeProvider.model,
            sessionId,
            useMemory: true,
            thinkingEnabled: modelApi.thinkingEnabled,
            reasoningEffort: modelApi.reasoningEffort,
            interactionMode: "agent",
            projectRoot: worktree?.path,
            attachments: pending.map((a) => ({
              name: a.name,
              mime: a.mime,
              kind: a.kind,
              size: a.size,
              dataBase64: a.dataBase64 ?? null,
              localPath: a.localPath ?? null,
            })),
          },
        });
        window.setTimeout(dispatchSessionsChanged, 120);
        return true;
      } catch (err) {
        const errMsg = String(err);
        finish("error", errMsg);
        if (worktree) {
          void invoke("cleanup_task_worktree", {
            path: worktree.path,
            repoRoot: worktree.repoRoot,
            branch: worktree.branch,
          }).catch(() => {});
        }
        setMessages((prev) =>
          prev.map((m) =>
            m.id === assistantId
              ? {
                  ...m,
                  content: errMsg,
                  error: true,
                  turnStatus: "error",
                  generationStartedAt: undefined,
                }
              : m,
          ),
        );
        showTransientToast(errMsg, { tone: "error" });
        return false;
      }
    },
    [cleanupTaskStream, enqueueToken, flushToken],
  );

  const clearSettledParallel = useCallback(() => {
    setParallelTasks((prev) => {
      for (const task of prev) {
        if (isParallelTaskActive(task.status)) continue;
        if (task.worktree) {
          void invoke("cleanup_task_worktree", {
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
