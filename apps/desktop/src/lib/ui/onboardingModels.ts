import { isChatModelId } from "../model/autoModelSelect.ts";
import type { ModelInfo } from "../../types";

/** Use the app's chat-model filter; never synthesize a default not returned by the provider. */
export function onboardingModelOptions(
  models: Pick<ModelInfo, "id" | "display_name">[],
) {
  const seen = new Set<string>();
  return models.flatMap((model) => {
    const id = model.id.trim();
    if (
      !id ||
      seen.has(id) ||
      !isChatModelId(id) ||
      /^(gpt-image|sora|veo)([-/]|$)/i.test(id)
    )
      return [];
    seen.add(id);
    const name = model.display_name?.trim();
    return [{ value: id, label: name && name !== id ? `${name} · ${id}` : id }];
  });
}
