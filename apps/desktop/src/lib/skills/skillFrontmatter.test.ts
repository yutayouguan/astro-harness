import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { splitSkillFrontmatter } from "./skillFrontmatter.ts";

describe("splitSkillFrontmatter", () => {
  it("returns null frontmatter when no fence", () => {
    const r = splitSkillFrontmatter("# Hello\n");
    assert.equal(r.frontmatter, null);
    assert.equal(r.body, "# Hello\n");
  });

  it("parses name and description", () => {
    const src =
      "---\nname: algorithmic-poster-philosophy\ndescription: Make posters.\n---\n# Body\n";
    const r = splitSkillFrontmatter(src);
    assert.equal(r.frontmatter?.name, "algorithmic-poster-philosophy");
    assert.equal(r.frontmatter?.description, "Make posters.");
    assert.equal(r.body, "# Body\n");
  });

  it("parses block scalar description", () => {
    const src =
      "---\nname: demo\ndescription: |\n  Line one\n  Line two\n---\nText\n";
    const r = splitSkillFrontmatter(src);
    assert.equal(r.frontmatter?.description, "Line one\nLine two");
    assert.equal(r.body, "Text\n");
  });

  it("collects unknown keys as extras", () => {
    const src = "---\nname: x\ncompatibility: Claude\n---\n";
    const r = splitSkillFrontmatter(src);
    assert.deepEqual(r.frontmatter?.extras, [
      { key: "compatibility", value: "Claude" },
    ]);
  });
});
