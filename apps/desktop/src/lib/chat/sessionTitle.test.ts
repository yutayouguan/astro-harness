import assert from "node:assert/strict";
import test from "node:test";
import {
  pendingSessionTitle,
  sessionTitleDisplay,
  visibleSessionTitle,
} from "./sessionTitle.ts";

test("hides the generated short id from duplicate cron session titles", () => {
  assert.equal(
    visibleSessionTitle(
      "定时任务 · test · 737536f4",
      "737536f4-6588-4cbc-99a2-7abc20d49b15",
    ),
    "定时任务 · test",
  );
});

test("keeps normal titles and unrelated suffixes intact", () => {
  assert.equal(
    visibleSessionTitle("test · 737536f4", "737536f4-rest"),
    "test · 737536f4",
  );
  assert.equal(
    visibleSessionTitle("定时任务 · test · abcdef12", "737536f4-rest"),
    "定时任务 · test · abcdef12",
  );
});

test("pending title collapses whitespace and truncates by code points", () => {
  assert.equal(pendingSessionTitle("  hello\n\n  world  "), "hello world");
  assert.equal(pendingSessionTitle("abcdefghij", 5), "abcde");
  assert.equal(pendingSessionTitle("   \n\t "), "");
});

test("cron session titles drop the prefix and flag the icon", () => {
  assert.deepEqual(
    sessionTitleDisplay("定时任务 · 每日舆情早报", "abcd1234-rest"),
    { isCron: true, title: "每日舆情早报" },
  );
  assert.deepEqual(
    sessionTitleDisplay(
      "定时任务 · test · 737536f4",
      "737536f4-6588-4cbc-99a2-7abc20d49b15",
    ),
    { isCron: true, title: "test" },
  );
  assert.deepEqual(sessionTitleDisplay("定时任务 · ", "abcd1234-rest"), {
    isCron: true,
    title: "",
  });
});

test("normal titles keep their text and drop the cron flag", () => {
  assert.deepEqual(sessionTitleDisplay("美化 AI 回答面板", "abcd1234-rest"), {
    isCron: false,
    title: "美化 AI 回答面板",
  });
  assert.deepEqual(
    sessionTitleDisplay("帮我写一个定时任务脚本", "abcd1234-rest"),
    { isCron: false, title: "帮我写一个定时任务脚本" },
  );
});
