import type { ModelInfo } from "../../types";

export type ToolLoadingMode = "auto" | "always" | "on_demand";
export type ToolLoadingSettings = {
  modes: Record<string, ToolLoadingMode>;
  adjustableToolsets: string[];
};

export function toolLoadingStatus(
  mode: ToolLoadingMode,
  defaultExposure: string | undefined,
  enabled: boolean,
  model: ModelInfo | null | undefined,
): "disabled" | "direct" | "deferred" | "fallback" | "unknown" | "codeMode" {
  if (!enabled) return "disabled";
  if (model?.tool_mode === "code_mode_only") return "codeMode";
  const deferred =
    mode === "on_demand" || (mode === "auto" && defaultExposure === "deferred");
  if (!deferred) return defaultExposure ? "direct" : "unknown";
  if (model?.profile?.supports_search_tool === false) return "fallback";
  if (model?.profile?.supports_search_tool === true) return "deferred";
  return "unknown";
}
