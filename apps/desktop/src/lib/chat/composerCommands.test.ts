import { test } from "node:test";
import assert from "node:assert/strict";
import { parseSlashInput, resolveBuiltinSlash } from "./composerCommands.ts";

test("resolves builtins and aliases", () => {
  assert.equal(resolveBuiltinSlash("new")?.action, "new_chat");
  assert.equal(resolveBuiltinSlash("reset")?.action, "new_chat");
  assert.equal(resolveBuiltinSlash("hel")?.name, "help");
  assert.equal(resolveBuiltinSlash("mcp")?.action, "nav_mcp");
  assert.equal(resolveBuiltinSlash("retry"), null);
  assert.equal(resolveBuiltinSlash("regenerate"), null);
});

test("parses skill slash before prefix match", () => {
  const parsed = parseSlashInput("/skills-demo foo", ["skills-demo"]);
  assert.equal(parsed?.action, "insert_skill");
  assert.equal(parsed?.skillName, "skills-demo");
  assert.equal(parsed?.args, "foo");
});

test("parses builtin with args", () => {
  const parsed = parseSlashInput("/mode");
  assert.equal(parsed?.action, "mode");
});

test("returns null for plain text", () => {
  assert.equal(parseSlashInput("hello"), null);
});
