import { describe, expect, it } from "vitest";
import {
  parseSlashInput,
  resolveBuiltinSlash,
} from "./composerCommands.ts";

describe("composerCommands", () => {
  it("resolves builtins and aliases", () => {
    expect(resolveBuiltinSlash("new")?.action).toBe("new_chat");
    expect(resolveBuiltinSlash("reset")?.action).toBe("new_chat");
    expect(resolveBuiltinSlash("hel")?.name).toBe("help");
    expect(resolveBuiltinSlash("mcp")?.action).toBe("nav_mcp");
  });

  it("parses skill slash before prefix match", () => {
    const parsed = parseSlashInput("/skills-demo foo", ["skills-demo"]);
    expect(parsed?.action).toBe("insert_skill");
    expect(parsed?.skillName).toBe("skills-demo");
    expect(parsed?.args).toBe("foo");
  });

  it("parses builtin with args", () => {
    const parsed = parseSlashInput("/mode");
    expect(parsed?.action).toBe("mode");
  });

  it("returns null for plain text", () => {
    expect(parseSlashInput("hello")).toBeNull();
  });
});
