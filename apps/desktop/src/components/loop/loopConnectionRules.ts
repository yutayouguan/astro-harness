import type { Connection, Node as RFNode } from "@xyflow/react";

const TRIGGER_TYPES: Set<string> = new Set([
  "manual_trigger",
  "scheduled_trigger",
  "webhook_trigger",
  "email_trigger",
  "file_watch_trigger",
]);

const OUTPUT_TYPES: Set<string> = new Set(["output"]);

export function isValidConnection(
  connection: Connection,
  nodes: RFNode[],
): boolean {
  if (!connection.source || !connection.target) return false;
  if (connection.source === connection.target) return false;

  const sourceNode = nodes.find((n) => n.id === connection.source);
  const targetNode = nodes.find((n) => n.id === connection.target);
  if (!sourceNode || !targetNode) return false;

  const sourceType =
    ((sourceNode.data as Record<string, unknown>).nodeType as string) ?? "";
  const targetType =
    ((targetNode.data as Record<string, unknown>).nodeType as string) ?? "";

  // 触发器不能作为连接目标
  if (TRIGGER_TYPES.has(targetType)) return false;
  // 输出节点不能作为连接源
  if (OUTPUT_TYPES.has(sourceType)) return false;
  // 触发器之间不能连接
  if (TRIGGER_TYPES.has(sourceType) && TRIGGER_TYPES.has(targetType))
    return false;

  return true;
}
