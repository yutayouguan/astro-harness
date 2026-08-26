import type { IconNode as LucideIconNode } from "lucide";
import type { IconInput } from "morphicons/react";

export type MorphableIcon = IconInput | LucideIconNode;

/** Convert lucide's root SVG tuple into the child-node list morphicons consumes. */
export function toMorphIconInput(icon: MorphableIcon): IconInput {
  if (Array.isArray(icon) && icon[0] === "svg" && Array.isArray(icon[2])) {
    return icon[2] as IconInput;
  }
  return icon as IconInput;
}
