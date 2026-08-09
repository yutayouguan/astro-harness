import { useCallback, useRef } from "react";
import type { Node as RFNode, Edge as RFEdge } from "@xyflow/react";

interface Snapshot {
  nodes: RFNode[];
  edges: RFEdge[];
}

const MAX_HISTORY = 50;

export function useLoopHistory(
  setNodes: (updater: (nodes: RFNode[]) => RFNode[]) => void,
  setEdges: (updater: (edges: RFEdge[]) => RFEdge[]) => void,
  getNodes: () => RFNode[],
  getEdges: () => RFEdge[],
) {
  const undoStack = useRef<Snapshot[]>([]);
  const redoStack = useRef<Snapshot[]>([]);
  const isUndoing = useRef(false);

  const pushSnapshot = useCallback(() => {
    if (isUndoing.current) return;
    const snap: Snapshot = {
      nodes: getNodes().map((n) => ({ ...n, position: { ...n.position } })),
      edges: getEdges().map((e) => ({ ...e })),
    };
    undoStack.current.push(snap);
    if (undoStack.current.length > MAX_HISTORY) undoStack.current.shift();
    redoStack.current = [];
  }, [getNodes, getEdges]);

  const undo = useCallback(() => {
    const snap = undoStack.current.pop();
    if (!snap) return;
    isUndoing.current = true;
    redoStack.current.push({
      nodes: getNodes().map((n) => ({ ...n, position: { ...n.position } })),
      edges: getEdges().map((e) => ({ ...e })),
    });
    setNodes(() => snap.nodes);
    setEdges(() => snap.edges);
    isUndoing.current = false;
  }, [getNodes, getEdges, setNodes, setEdges]);

  const redo = useCallback(() => {
    const snap = redoStack.current.pop();
    if (!snap) return;
    isUndoing.current = true;
    undoStack.current.push({
      nodes: getNodes().map((n) => ({ ...n, position: { ...n.position } })),
      edges: getEdges().map((e) => ({ ...e })),
    });
    setNodes(() => snap.nodes);
    setEdges(() => snap.edges);
    isUndoing.current = false;
  }, [getNodes, getEdges, setNodes, setEdges]);

  const canUndo = () => undoStack.current.length > 0;
  const canRedo = () => redoStack.current.length > 0;

  return { pushSnapshot, undo, redo, canUndo, canRedo };
}
