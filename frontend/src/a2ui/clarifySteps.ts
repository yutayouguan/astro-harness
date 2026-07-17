/** 解析 ClarifyWizard 的 steps 载荷（无 React 依赖）。 */

export type ClarifyWizardStep = {
  id: string;
  question: string;
  options: string[];
};

function optionValue(opt: string | { label?: string; value?: string }): string {
  if (typeof opt === "string") return opt;
  return opt.value ?? opt.label ?? "";
}

export function parseClarifySteps(raw: unknown): ClarifyWizardStep[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .map((item, i) => {
      if (!item || typeof item !== "object") return null;
      const o = item as Record<string, unknown>;
      const question = typeof o.question === "string" ? o.question.trim() : "";
      if (!question) return null;
      const id =
        typeof o.id === "string" && o.id.trim()
          ? o.id.trim()
          : `q${i}`;
      const optionsRaw = Array.isArray(o.options) ? o.options : [];
      const options = optionsRaw
        .map((opt) => {
          if (typeof opt === "string") return opt.trim();
          if (opt && typeof opt === "object") {
            const v = optionValue(opt as { label?: string; value?: string });
            return v.trim();
          }
          return "";
        })
        .filter(Boolean);
      return {
        id,
        question,
        options: options.length ? options : ["继续"],
      } satisfies ClarifyWizardStep;
    })
    .filter((s): s is ClarifyWizardStep => s != null);
}
