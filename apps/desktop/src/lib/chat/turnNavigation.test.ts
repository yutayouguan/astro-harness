import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { buildTurnPreviews, compactEntryPreview } from "./turnNavigation.ts";

describe("buildTurnPreviews", () => {
  it("groups each user question with its following assistant answer", () => {
    const turns = buildTurnPreviews(
      [
        { id: "welcome", role: "assistant", content: "Welcome" },
        { id: "q1", role: "user", content: "Please **review** this" },
        { id: "a1", role: "assistant", content: "Found one issue." },
        { id: "q2", role: "user", content: "Fix it" },
        { id: "a2", role: "assistant", content: "Done." },
      ],
      "Question",
      "Waiting for answer",
    );

    assert.deepEqual(turns, [
      {
        id: "q1",
        targetEntryId: "q1",
        entryIds: ["q1", "a1"],
        question: "Please review this",
        answer: "Found one issue.",
      },
      {
        id: "q2",
        targetEntryId: "q2",
        entryIds: ["q2", "a2"],
        question: "Fix it",
        answer: "Done.",
      },
    ]);
  });

  it("keeps an unfinished question as one pending turn", () => {
    const turns = buildTurnPreviews(
      [{ id: "q1", role: "user", content: "Next?" }],
      "Question",
      "Waiting for answer",
    );
    assert.equal(turns[0]?.answer, "Waiting for answer");
  });
});

describe("compactEntryPreview", () => {
  it("removes markdown decoration and bounds long previews", () => {
    assert.equal(
      compactEntryPreview("# Result\n[docs](https://example.com)"),
      "Result docs",
    );
    assert.equal(compactEntryPreview("abcdefgh", 6), "abcde…");
  });

  it("removes raw tool-call protocol noise from the answer preview", () => {
    assert.equal(
      compactEntryPreview(
        '<tool_call>{"name":"exec_command"}</tool_call> 已完成检查，没有发现问题。',
      ),
      "已完成检查，没有发现问题。",
    );
  });
});
