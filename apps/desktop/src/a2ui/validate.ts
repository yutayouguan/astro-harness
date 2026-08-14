/** 轻量 A2UI 校验：未知组件不阻断整卡，由渲染层降级。 */

import {
  ALLOWED_COMPONENTS,
  type A2uiComponent,
  type A2uiOperation,
} from "./types";

export function parseOperations(raw: unknown): A2uiOperation[] {
  if (!Array.isArray(raw)) return [];
  return raw.filter((item) => item && typeof item === "object") as A2uiOperation[];
}

export function collectComponents(operations: A2uiOperation[]): A2uiComponent[] {
  const out: A2uiComponent[] = [];
  for (const op of operations) {
    const list = op.updateComponents?.components;
    if (!Array.isArray(list)) continue;
    for (const c of list) {
      if (c && typeof c.id === "string" && typeof c.component === "string") {
        out.push(c);
      }
    }
  }
  return out;
}

export function isKnownComponent(name: string): boolean {
  return ALLOWED_COMPONENTS.has(name);
}
