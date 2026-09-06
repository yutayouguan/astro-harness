export type ComposerContextKind = "agent" | "skill" | "mcp" | "file";

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

export function createFileComposerContextToken(
  path: string,
  description?: string,
): ComposerContextToken {
  const normalizedPath = path.trim();
  const baseName = normalizedPath.replace(/\\/g, "/").split("/").pop();
  return {
    id: normalizedPath,
    kind: "file",
    name: baseName || normalizedPath,
    description,
    path: normalizedPath,
  };
}

export function serializeComposerContext(
  tokens: ComposerContextToken[],
  input: string,
): string {
  const prefixes = tokens.map((token) => {
    if (token.kind === "skill") return `/${token.name}`;
    if (token.kind === "file") return `@${token.path || token.name}`;
    return `@${token.name}`;
  });
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
