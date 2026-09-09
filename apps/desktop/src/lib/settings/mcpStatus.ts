import type {
  McpRuntimeState,
  McpRuntimeStatus,
} from "../../hooks/providers/useMcpTools";

/** Configured off wins over a stale runtime snapshot; absent status is not a connection. */
export function mcpDisplayState(
  enabled: boolean,
  runtime?: Pick<McpRuntimeStatus, "status">,
  reconnecting = false,
): McpRuntimeState {
  if (!enabled) return "disabled";
  if (reconnecting) return "connecting";
  return runtime?.status ?? "unknown";
}
