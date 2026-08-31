import type { ChatMessage, PendingInterrupt, UiSurface } from "../../types.ts";

export type ComposerClarifySurface = {
  messageId: string;
  surface: UiSurface;
};

export function isClarifySurface(surface: UiSurface): boolean {
  return surface.operations.some((operation) => {
    if (!operation || typeof operation !== "object") return false;
    const components = (
      operation as {
        updateComponents?: { components?: unknown };
      }
    ).updateComponents?.components;
    return (
      Array.isArray(components) &&
      components.some(
        (component) =>
          Boolean(component) &&
          typeof component === "object" &&
          (component as { component?: unknown }).component === "ClarifyWizard",
      )
    );
  });
}

function surfaceMatchesInterrupts(
  messageId: string,
  surface: UiSurface,
  pendingInterrupts: PendingInterrupt[],
): boolean {
  const pendingIds = new Set(pendingInterrupts.map((item) => item.id));
  return (
    pendingInterrupts.some((item) => item.assistantMessageId === messageId) ||
    Boolean(surface.interrupts?.some((item) => pendingIds.has(item.id)))
  );
}

export function findComposerClarifySurface(
  messages: ChatMessage[],
  pendingInterrupts: PendingInterrupt[],
): ComposerClarifySurface | null {
  if (pendingInterrupts.length === 0) return null;

  let fallback: ComposerClarifySurface | null = null;
  let hasExplicitLink = pendingInterrupts.some((interrupt) =>
    Boolean(interrupt.assistantMessageId),
  );
  for (
    let messageIndex = messages.length - 1;
    messageIndex >= 0;
    messageIndex -= 1
  ) {
    const message = messages[messageIndex];
    if (!message || message.role !== "assistant") continue;

    const surfaces = message.uiSurfaces ?? [];
    for (
      let surfaceIndex = surfaces.length - 1;
      surfaceIndex >= 0;
      surfaceIndex -= 1
    ) {
      const surface = surfaces[surfaceIndex];
      if (
        !surface ||
        surface.status !== "active" ||
        !isClarifySurface(surface)
      ) {
        continue;
      }

      const candidate = { messageId: message.id, surface };
      hasExplicitLink ||= Boolean(surface.interrupts?.length);
      if (surfaceMatchesInterrupts(message.id, surface, pendingInterrupts)) {
        return candidate;
      }
      fallback ??= candidate;
    }
  }

  // Older snapshots may not carry assistantMessageId or surface interrupt ids.
  return hasExplicitLink ? null : fallback;
}
