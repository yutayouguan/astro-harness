import { describe, expect, it } from "vitest";
import { splitSkillFrontmatter } from "./skillFrontmatter";

describe("splitSkillFrontmatter", () => {
  it("returns null frontmatter when no fence", () => {
    const r = splitSkillFrontmatter("# Hello\n");
    expect(r.frontmatter).toBeNull();
    expect(r.body).toBe("# Hello\n");
  });

  it("parses name and description", () => {
    const src =
      "---\nname: algorithmic-poster-philosophy\ndescription: Make posters.\n---\n# Body\n";
    const r = splitSkillFrontmatter(src);
    expect(r.frontmatter?.name).toBe("algorithmic-poster-philosophy");
    expect(r.frontmatter?.description).toBe("Make posters.");
    expect(r.body).toBe("# Body\n");
  });

  it("parses block scalar description", () => {
    const src =
      "---\nname: demo\ndescription: |\n  Line one\n  Line two\n---\nText\n";
    const r = splitSkillFrontmatter(src);
    expect(r.frontmatter?.description).toBe("Line one\nLine two");
    expect(r.body).toBe("Text\n");
  });

  it("collects unknown keys as extras", () => {
    const src = "---\nname: x\ncompatibility: Claude\n---\n";
    const r = splitSkillFrontmatter(src);
    expect(r.frontmatter?.extras).toEqual([
      { key: "compatibility", value: "Claude" },
    ]);
  });
});
