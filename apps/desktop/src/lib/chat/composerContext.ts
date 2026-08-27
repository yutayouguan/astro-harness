export type ComposerContextKind = "agent" | "skill" | "mcp";

export type ComposerContextToken = {
  id: string;
  kind: ComposerContextKind;
  name: string;
  description?: string;
  path?: string;
};

export function addComposerContextToken(
  current: ComposerContextToken[],
  next: ComposerContextToken,
): ComposerContextToken[] {
  const duplicate = current.some(
    (item) => item.kind === next.kind && item.id === next.id,
  );
  return duplicate ? current : [...current, next];
}

export function serializeComposerContext(
  tokens: ComposerContextToken[],
  input: string,
): string {
  const prefixes = tokens.map((token) =>
    token.kind === "skill" ? `/${token.name}` : `@${token.name}`,
  );
  return [...prefixes, input.trim()].filter(Boolean).join(" ");
}

export function removeTriggerText(
  input: string,
  start: number,
  end: number,
): string {
  return `${input.slice(0, start)}${input.slice(end)}`
    .replace(/[ \t]{2,}/g, " ")
    .replace(/^ /, "");
}
