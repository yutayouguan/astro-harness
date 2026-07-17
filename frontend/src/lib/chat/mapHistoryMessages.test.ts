import assert from "node:assert/strict";
import test from "node:test";
import { mapHistoryMessages } from "./mapHistoryMessages.ts";

test("mapHistoryMessages restores activity media from history DTO", () => {
  const msgs = mapHistoryMessages([
    {
      id: "db-1",
      role: "assistant",
      content: "done",
      activities: [
        {
          id: "c1",
          kind: "tool",
          title: "image_gen",
          output: "图片已生成：generated/images/a.jpg",
          status: "done",
          media: [{ kind: "image", path: "generated/images/a.jpg" }],
        },
      ],
    },
  ]);
  assert.equal(msgs.length, 1);
  assert.deepEqual(msgs[0]!.activities?.[0]?.media, [
    { kind: "image", path: "generated/images/a.jpg" },
  ]);
});

test("mapHistoryMessages drops invalid media entries", () => {
  const msgs = mapHistoryMessages([
    {
      id: "db-1",
      role: "assistant",
      content: "",
      activities: [
        {
          id: "c1",
          kind: "tool",
          title: "image_gen",
          status: "done",
          media: [
            { kind: "image", path: "ok.jpg" },
            { kind: "nope", path: "x.jpg" },
            { kind: "image", path: "  " },
          ],
        },
      ],
    },
  ]);
  assert.deepEqual(msgs[0]!.activities?.[0]?.media, [
    { kind: "image", path: "ok.jpg" },
  ]);
});
