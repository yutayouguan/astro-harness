/** Replace a divergent streamed draft with the canonical thread snapshot text. */
export function reconcileAssistantText(_streamed: string, canonical: string): string {
  return canonical;
}

export function consumeBufferedTextReconcile(
  _draft: string,
  _buffered: string,
  canonical: string,
): { content: string; buffered: string } {
  return { content: canonical, buffered: "" };
}
