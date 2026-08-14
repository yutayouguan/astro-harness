import type { ModelCapabilities } from "../../types";

/** 是否显示推理控件：有 caps 用 reasoning；未知则回退 deepseek / google。 */
export function shouldShowThinkingControls(input: {
  capabilities?: ModelCapabilities | null;
  backendId?: string | null;
}): boolean {
  if (input.capabilities != null) {
    return Boolean(input.capabilities.reasoning);
  }
  const backend = (input.backendId ?? "").toLowerCase();
  return backend === "deepseek" || backend === "google";
}
