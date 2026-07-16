/** 解析 SKILL.md 等文件开头的 YAML frontmatter（轻量，非完整 YAML）。 */

export type SkillFrontmatterExtra = { key: string; value: string };

export type SkillFrontmatter = {
  name?: string;
  description?: string;
  extras: SkillFrontmatterExtra[];
  raw: string;
};

export type SplitSkillFrontmatter = {
  frontmatter: SkillFrontmatter | null;
  body: string;
};

function stripQuotes(v: string): string {
  const t = v.trim();
  if (
    (t.startsWith('"') && t.endsWith('"')) ||
    (t.startsWith("'") && t.endsWith("'"))
  ) {
    return t.slice(1, -1);
  }
  return t;
}

/** 解析 frontmatter 块内的简单键值（支持 `|` / `>` 缩进多行）。 */
function parseFrontmatterFields(block: string): Omit<SkillFrontmatter, "raw"> {
  const lines = block.replace(/^\uFEFF/, "").split(/\r?\n/);
  let name: string | undefined;
  let description: string | undefined;
  const extras: SkillFrontmatterExtra[] = [];

  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? "";
    i += 1;
    if (!line.trim() || line.trimStart().startsWith("#")) continue;

    const m = /^([A-Za-z_][\w-]*)\s*:\s*(.*)$/.exec(line);
    if (!m) continue;

    const key = m[1]!;
    let value = (m[2] ?? "").trimEnd();

    if (value === "|" || value === ">" || value === "|-" || value === ">-") {
      const parts: string[] = [];
      while (i < lines.length) {
        const next = lines[i] ?? "";
        if (next.length === 0) {
          parts.push("");
          i += 1;
          continue;
        }
        if (/^\s/.test(next)) {
          parts.push(next.replace(/^\s+/, ""));
          i += 1;
          continue;
        }
        break;
      }
      value = parts.join("\n").trim();
    } else {
      value = stripQuotes(value);
    }

    if (key === "name") name = value;
    else if (key === "description") description = value;
    else extras.push({ key, value });
  }

  return { name, description, extras };
}

/**
 * 若内容以 `---` 开头，拆出 frontmatter 与正文；否则 frontmatter 为 null。
 */
export function splitSkillFrontmatter(content: string): SplitSkillFrontmatter {
  const text = content.replace(/^\uFEFF/, "");
  if (!text.startsWith("---")) {
    return { frontmatter: null, body: content };
  }

  const afterOpen = text.slice(3);
  // 允许首行 `---` 后直接换行；结束标记为独立一行的 ---
  const endMatch = /\r?\n---[ \t]*(?:\r?\n|$)/.exec(afterOpen);
  if (!endMatch || endMatch.index == null) {
    return { frontmatter: null, body: content };
  }

  const raw = afterOpen.slice(0, endMatch.index).replace(/^\r?\n/, "");
  const bodyStart = 3 + endMatch.index + endMatch[0].length;
  const body = text.slice(bodyStart).replace(/^\r?\n/, "");
  const fields = parseFrontmatterFields(raw);

  return {
    frontmatter: { ...fields, raw },
    body,
  };
}
