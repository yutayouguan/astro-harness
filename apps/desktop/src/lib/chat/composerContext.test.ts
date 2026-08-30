import assert from "node:assert/strict";
import test from "node:test";
import {
  addComposerContextToken,
  createFileComposerContextToken,
  removeTriggerText,
  serializeComposerContext,
  type ComposerContextToken,
} from "./composerContext.ts";

const skill: ComposerContextToken = {
  id: "skill-pdf",
  kind: "skill",
  name: "pdf",
};

test("composer context tokens are deduplicated by kind and id", () => {
  assert.deepEqual(addComposerContextToken([skill], skill), [skill]);
  assert.equal(
    addComposerContextToken([skill], { id: "mcp-pdf", kind: "mcp", name: "pdf" }).length,
    2,
  );
});

test("composer context serializes to the existing skill and mention protocol", () => {
  assert.equal(
    serializeComposerContext(
      [
        skill,
        { id: "mcp-browser", kind: "mcp", name: "browser" },
        { id: "agent-reviewer", kind: "agent", name: "reviewer" },
      ],
      "检查这份文件",
    ),
    "/pdf @browser @reviewer 检查这份文件",
  );
});

test("file context keeps its path behind a compact composer token", () => {
  const file = createFileComposerContextToken(
    "/tmp/generated/fetch_title.py",
    "代码",
  );
  assert.deepEqual(file, {
    id: "/tmp/generated/fetch_title.py",
    kind: "file",
    name: "fetch_title.py",
    description: "代码",
    path: "/tmp/generated/fetch_title.py",
  });
  assert.equal(
    serializeComposerContext([file], "继续修改"),
    "@/tmp/generated/fetch_title.py 继续修改",
  );
});

test("selecting a token removes the typed trigger without eating surrounding text", () => {
  assert.equal(removeTriggerText("请用 /pd 完成", 3, 6), "请用 完成");
  assert.equal(removeTriggerText("@bro 检查", 0, 4), "检查");
});
