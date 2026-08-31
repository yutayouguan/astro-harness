/**
 * 生成中的文件实时预览状态：解析 apply_patch 等写工具的流式参数，累积 path/content。
 *
 * 数据来源是 useSend 的 tool_call_delta / tool_call 事件。为避免频繁重渲染
 * （尤其是 HTML iframe 重挂），状态更新经 ~90ms 节流。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { parsePartialFileWrite } from "../../lib/chat/parsePartialFileWrite";

export type GeneratingPreviewKind = "html" | "code";

export type GeneratingPreview = {
  /** 关联的工具调用 key（id 或 idx-N）。 */
  toolKey: string;
  /** 工作区相对/绝对路径（解析到才有）。 */
  path: string | null;
  /** 用于语言高亮 / 标题的文件名。 */
  filename: string | null;
  /** 已到达的内容（流式追加）。 */
  content: string;
  kind: GeneratingPreviewKind;
  status: "streaming" | "done";
};

export type GeneratingPreviewApi = {
  onToolDelta: (d: {
    index: number;
    id?: string;
    name?: string;
    arguments?: string;
  }) => void;
  onToolCall: (c: {
    id?: string;
    name?: string;
    arguments_json?: string;
    result?: string;
  }) => void;
  onStreamEnd: () => void;
  reset: () => void;
};

function basenameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

function keyFor(index: number, id?: string): string {
  const trimmed = id?.trim();
  return trimmed || `idx-${index}`;
}

export function useGeneratingPreview(opts: { onActivate?: () => void }) {
  const [preview, setPreview] = useState<GeneratingPreview | null>(null);

  const argsRef = useRef<Map<string, string>>(new Map());
  const nameRef = useRef<Map<string, string>>(new Map());
  const activeKeyRef = useRef<string | null>(null);
  const activatedRef = useRef(false);
  const pendingRef = useRef<GeneratingPreview | null>(null);
  const throttleRef = useRef<number | null>(null);
  const optsRef = useRef(opts);
  optsRef.current = opts;

  useEffect(() => {
    return () => {
      if (throttleRef.current != null) {
        window.clearTimeout(throttleRef.current);
      }
    };
  }, []);

  const flush = useCallback(() => {
    throttleRef.current = null;
    if (pendingRef.current) {
      setPreview(pendingRef.current);
      pendingRef.current = null;
    }
  }, []);

  const schedule = useCallback(
    (next: GeneratingPreview) => {
      pendingRef.current = next;
      if (throttleRef.current == null) {
        throttleRef.current = window.setTimeout(flush, 90);
      }
    },
    [flush],
  );

  const commit = useCallback((next: GeneratingPreview | null) => {
    if (throttleRef.current != null) {
      window.clearTimeout(throttleRef.current);
      throttleRef.current = null;
    }
    pendingRef.current = null;
    setPreview(next);
  }, []);

  const reset = useCallback(() => {
    argsRef.current.clear();
    nameRef.current.clear();
    activeKeyRef.current = null;
    activatedRef.current = false;
    commit(null);
  }, [commit]);

  const onToolDelta = useCallback(
    (d: { index: number; id?: string; name?: string; arguments?: string }) => {
      const key = keyFor(d.index, d.id);
      const args = (argsRef.current.get(key) ?? "") + (d.arguments ?? "");
      argsRef.current.set(key, args);
      if (d.name && d.name.trim()) nameRef.current.set(key, d.name.trim());

      const name = (nameRef.current.get(key) ?? "").toLowerCase();
      // 名称已知但不是可预览的写工具：跳过
      if (name && name !== "apply_patch") return;

      const parsed = parsePartialFileWrite(args);
      // 需要 content 字段才值得预览（read/list/patch 无 content）
      if (!parsed || parsed.content === undefined) return;

      const path = parsed.path ?? null;
      const filename = path ? basenameOf(path) : null;
      const kind: GeneratingPreviewKind =
        path && /\.html?$/i.test(path) ? "html" : "code";

      activeKeyRef.current = key;
      schedule({
        toolKey: key,
        path,
        filename,
        content: parsed.content,
        kind,
        status: "streaming",
      });

      if (!activatedRef.current) {
        activatedRef.current = true;
        optsRef.current.onActivate?.();
      }
    },
    [schedule],
  );

  const onToolCall = useCallback(
    (c: {
      id?: string;
      name?: string;
      arguments_json?: string;
      result?: string;
    }) => {
      const activeKey = activeKeyRef.current;
      if (!activeKey) return;
      const parsed = parsePartialFileWrite(c.arguments_json);
      const finalPath = parsed?.path ?? null;
      const finalContent = parsed?.content;
      // 落定最终态：优先用刚到的完整参数，其次沿用节流中的 pending / 现值
      const base = pendingRef.current;
      if (throttleRef.current != null) {
        window.clearTimeout(throttleRef.current);
        throttleRef.current = null;
      }
      pendingRef.current = null;
      setPreview((prev) => {
        const cur =
          base && base.toolKey === activeKey
            ? base
            : prev && prev.toolKey === activeKey
              ? prev
              : null;
        if (!cur) return prev;
        const path = finalPath ?? cur.path;
        const filename = path ? basenameOf(path) : cur.filename;
        const kind: GeneratingPreviewKind =
          path && /\.html?$/i.test(path) ? "html" : cur.kind;
        return {
          ...cur,
          path,
          filename,
          content: finalContent ?? cur.content,
          kind,
          status: "done",
        };
      });
      argsRef.current.delete(activeKey);
      nameRef.current.delete(activeKey);
      activeKeyRef.current = null;
    },
    [],
  );

  const onStreamEnd = useCallback(() => {
    if (throttleRef.current != null) {
      window.clearTimeout(throttleRef.current);
      throttleRef.current = null;
    }
    if (pendingRef.current) {
      setPreview(pendingRef.current);
      pendingRef.current = null;
    }
    setPreview((prev) => (prev ? { ...prev, status: "done" } : prev));
    argsRef.current.clear();
    nameRef.current.clear();
    activeKeyRef.current = null;
  }, []);

  return {
    preview,
    api: {
      onToolDelta,
      onToolCall,
      onStreamEnd,
      reset,
    } satisfies GeneratingPreviewApi,
  };
}
