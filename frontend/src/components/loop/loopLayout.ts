import type { LoopNodeDto, LoopEdgeDto } from "./loopTypes";

const LAYER_GAP_X = 240;
const NODE_GAP_Y = 90;

export function layoutNodes(
  nodes: LoopNodeDto[],
  edges: LoopEdgeDto[],
): Map<string, { x: number; y: number }> {
  const ids = new Set(nodes.map((n) => n.id));
  const children: Map<string, string[]> = new Map();
  const inDeg: Map<string, number> = new Map();

  for (const id of ids) {
    children.set(id, []);
    inDeg.set(id, 0);
  }
  for (const e of edges) {
    if (!ids.has(e.source) || !ids.has(e.target)) continue;
    children.get(e.source)!.push(e.target);
    inDeg.set(e.target, (inDeg.get(e.target) ?? 0) + 1);
  }

  const layers: string[][] = [];
  const depth: Map<string, number> = new Map();

  const queue: string[] = [];
  for (const id of ids) {
    if (inDeg.get(id) === 0) {
      queue.push(id);
      depth.set(id, 0);
    }
  }

  while (queue.length > 0) {
    const cur = queue.shift()!;
    const d = depth.get(cur)!;
    for (const child of children.get(cur)!) {
      const prev = depth.get(child);
      if (prev === undefined || d + 1 > prev) {
        depth.set(child, d + 1);
      }
      inDeg.set(child, inDeg.get(child)! - 1);
      if (inDeg.get(child) === 0) {
        queue.push(child);
      }
    }
  }

  for (const id of ids) {
    if (!depth.has(id)) depth.set(id, (layers.length || 1) - 1);
  }

  for (const [id, d] of depth) {
    while (layers.length <= d) layers.push([]);
    layers[d].push(id);
  }

  const positions = new Map<string, { x: number; y: number }>();
  for (let col = 0; col < layers.length; col++) {
    const layer = layers[col];
    const totalHeight = (layer.length - 1) * NODE_GAP_Y;
    const startY = -totalHeight / 2;
    for (let row = 0; row < layer.length; row++) {
      positions.set(layer[row], {
        x: col * LAYER_GAP_X,
        y: startY + row * NODE_GAP_Y,
      });
    }
  }

  return positions;
}
