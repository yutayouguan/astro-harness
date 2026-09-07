import type { ChatActivity, TurnTokenUsage } from "../../types.ts";
import { isCodePath } from "../media/parseGeneratedMedia.ts";

export type ChatStreamMediaRef = {
  kind?: string;
  mime_type?: string;
  ref_kind?: string;
  ref_value?: string;
  label?: string;
  id?: string;
};

export type ChatStreamEventPayload = {
  type: string;
  content?: string;
  questions?: Array<{ title: string; options?: string[] }>;
  message?: string;
  id?: string;
  name?: string;
  arguments_json?: string;
  arguments?: string;
  result?: string;
  web_action?: ChatActivity["webAction"];
  web_page_title?: string;
  delta?: string;
  phase?: string;
  batch_id?: string;
  execution_mode?: "serial" | "parallel";
  media?: ChatStreamMediaRef[];
  file_changes?: ChatActivity["fileChanges"];
  operation?: string;
  detail?: string;
  outcome?: string;
  index?: number;
  prompt_tokens?: number;
  uncached_input_tokens?: number;
  completion_tokens?: number;
  total_tokens?: number;
  provider_total_tokens?: number;
  cache_read_tokens?: number;
  cache_write_tokens?: number;
  reasoning_tokens?: number;
  request_count?: number;
  cache_read_reported?: boolean;
  cache_write_reported?: boolean;
  reasoning_reported?: boolean;
  context_window?: number;
  estimated_total_tokens?: number;
  source?: string;
  latest_usage?: {
    input_tokens?: number;
    uncached_input_tokens?: number;
    output_tokens?: number;
    total_tokens?: number;
    provider_total_tokens?: number;
    cache_read_tokens?: number;
    cache_write_tokens?: number;
    reasoning_tokens?: number;
    cache_read_reported?: boolean;
    cache_write_reported?: boolean;
    reasoning_reported?: boolean;
  } | null;
  segments?: Array<{
    id: string;
    tokens: number;
    count?: number | null;
  }>;
  updated_at?: number;
  thread_id?: string;
  run_id?: string;
  message_id?: string;
  activity_type?: string;
  content_json?: string;
  replace?: boolean;
  outcome_type?: string;
  interrupts_json?: string;
  citations?: string;
  client_message_id?: string;
};

export function usageFromStreamEvent(
  payload: ChatStreamEventPayload,
): TurnTokenUsage {
  return {
    promptTokens: payload.prompt_tokens ?? 0,
    uncachedInputTokens:
      payload.uncached_input_tokens ?? payload.prompt_tokens ?? 0,
    completionTokens: payload.completion_tokens ?? 0,
    totalTokens: payload.total_tokens ?? 0,
    providerTotalTokens: payload.provider_total_tokens,
    cacheReadTokens: payload.cache_read_tokens ?? 0,
    cacheWriteTokens: payload.cache_write_tokens ?? 0,
    reasoningTokens: payload.reasoning_tokens ?? 0,
    requestCount: payload.request_count ?? 0,
    cacheReadReported: payload.cache_read_reported === true,
    cacheWriteReported: payload.cache_write_reported === true,
    reasoningReported: payload.reasoning_reported === true,
  };
}

export function toolActivityKind(name: string): ChatActivity["kind"] {
  const lower = name.toLowerCase();
  if (lower.startsWith("mcp_")) return "mcp";
  if (lower.startsWith("skill_") || lower.includes("skill")) return "skill";
  if (lower.includes("hook")) return "hook";
  return "tool";
}

export function mediaFromStreamEvent(
  media: ChatStreamMediaRef[] | undefined,
): ChatActivity["media"] | undefined {
  if (!Array.isArray(media)) return undefined;
  const normalized = media
    .map((item) => {
      const path = item.ref_value;
      let kind: NonNullable<ChatActivity["media"]>[number]["kind"] | null =
        item.kind === "image" ||
        item.kind === "video" ||
        item.kind === "audio" ||
        item.kind === "html"
          ? item.kind
          : null;
      if (!kind && item.kind === "file" && path) {
        if (/\.html?$/i.test(path)) kind = "html";
        else if (/\.(png|jpe?g|webp|gif|bmp|svg|avif)$/i.test(path))
          kind = "image";
        else if (/\.(mp4|webm|mov|mkv|m4v)$/i.test(path)) kind = "video";
        else if (/\.(wav|mp3|m4a|aac|ogg|flac|opus)$/i.test(path))
          kind = "audio";
        else if (isCodePath(path)) kind = "code";
      }
      return kind && path ? { kind, path } : null;
    })
    .filter(
      (item): item is NonNullable<ChatActivity["media"]>[number] =>
        item !== null,
    );
  return normalized.length > 0 ? normalized : undefined;
}
