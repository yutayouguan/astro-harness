/** 按 id 渲染 Lucide 图标。 */
import type { ComponentType } from "react";
import type { LucideProps } from "lucide-react";
import { resolveLucideIconById } from "../../lib/agent/lucideAgentIcons";

type Props = LucideProps & {
  /** Lucide kebab-case id，如 calendar-check */
  name?: string | null;
  fallback?: ComponentType<LucideProps>;
};

/** 将后端 emoji 字段（Lucide 图标名）渲染为对应图标 */
export default function LucideByName({
  name,
  fallback: Fallback,
  ...props
}: Props) {
  const Icon = resolveLucideIconById(name ?? undefined);
  if (Icon) {
    return <Icon aria-hidden {...props} />;
  }
  if (Fallback) {
    return <Fallback aria-hidden {...props} />;
  }
  return null;
}
