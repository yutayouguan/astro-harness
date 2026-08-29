export type UnifiedDiffLineKind =
  | "context"
  | "addition"
  | "deletion"
  | "hunk"
  | "meta";

export type UnifiedDiffLine = {
  kind: UnifiedDiffLineKind;
  content: string;
  oldLine: number | null;
  newLine: number | null;
};

const HUNK_HEADER = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/;

/** Convert a unified patch into rows suitable for the compact review surface. */
export function parseUnifiedDiff(patch: string): UnifiedDiffLine[] {
  let oldLine = 0;
  let newLine = 0;

  return patch.split("\n").flatMap<UnifiedDiffLine>((line): UnifiedDiffLine[] => {
    const hunk = HUNK_HEADER.exec(line);
    if (hunk) {
      oldLine = Number(hunk[1]);
      newLine = Number(hunk[2]);
      return [{ kind: "hunk", content: line, oldLine: null, newLine: null }];
    }
    if (
      line.startsWith("diff --git ") ||
      line.startsWith("index ") ||
      line.startsWith("--- ") ||
      line.startsWith("+++ ") ||
      line.startsWith("\\ No newline")
    ) {
      return [{ kind: "meta", content: line, oldLine: null, newLine: null }];
    }
    if (line.startsWith("+") && !line.startsWith("+++")) {
      const current = newLine++;
      return [{ kind: "addition", content: line.slice(1), oldLine: null, newLine: current }];
    }
    if (line.startsWith("-") && !line.startsWith("---")) {
      const current = oldLine++;
      return [{ kind: "deletion", content: line.slice(1), oldLine: current, newLine: null }];
    }
    if (line.startsWith(" ")) {
      const currentOld = oldLine++;
      const currentNew = newLine++;
      return [{
        kind: "context",
        content: line.slice(1),
        oldLine: currentOld,
        newLine: currentNew,
      }];
    }
    return line
      ? [{ kind: "meta", content: line, oldLine: null, newLine: null }]
      : [];
  });
}
