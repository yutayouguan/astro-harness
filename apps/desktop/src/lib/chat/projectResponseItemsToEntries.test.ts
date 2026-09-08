import assert from "node:assert/strict";
import test from "node:test";
import {
  coalesceConsecutiveAssistants,
  enrichTimelineDurations,
  projectResponseItemsToEntries,
  normalizeProjectedEntries,
  settleRestoredActivities,
} from "./projectResponseItemsToEntries.ts";

test("projectResponseItemsToEntries folds native call and output items at render time", () => {
  const messages = projectResponseItemsToEntries([
    {
      id: "1",
      timestamp: 1,
      item: {
        type: "message",
        role: "user",
        content: [{ type: "input_text", text: "run it" }],
      },
    },
    {
      id: "2",
      timestamp: 2,
      item: {
        type: "function_call",
        call_id: "call_1",
        name: "terminal",
        arguments: '{"command":"pwd"}',
      },
    },
    {
      id: "3",
      timestamp: 3,
      item: {
        type: "function_call_output",
        call_id: "call_1",
        name: "terminal",
        output: "ok",
      },
    },
  ]);

  assert.equal(messages.length, 2);
  assert.equal(messages[0]?.content, "run it");
  const activity = messages[1]?.activities?.[0];
  assert.equal(activity?.id, "call_1");
  assert.equal(activity?.title, "terminal");
  assert.equal(activity?.input, '{"command":"pwd"}');
  assert.equal(activity?.output, "ok");
  assert.equal(activity?.status, "done");
});

test("projectResponseItemsToEntries restores persisted terminal tool status", () => {
  const messages = projectResponseItemsToEntries([
    {
      id: "1",
      timestamp: 1,
      item: {
        type: "function_call",
        call_id: "call_1",
        name: "exec_command",
        arguments: '{"cmd":"dangerous"}',
      },
    },
    {
      id: "2",
      timestamp: 2,
      item: {
        type: "function_call_output",
        call_id: "call_1",
        name: "exec_command",
        output: "Permission denied by user",
        internal_chat_message_metadata_passthrough: {
          astro_tool_status: "declined",
        },
      },
    },
  ]);

  assert.equal(messages[0]?.activities?.[0]?.status, "declined");
});

test("projectResponseItemsToEntries preserves native tool namespaces", () => {
  const messages = projectResponseItemsToEntries([
    {
      id: "1",
      timestamp: 1,
      item: {
        type: "function_call",
        call_id: "call_1",
        namespace: "cron",
        name: "list",
        arguments: "{}",
      },
    },
    {
      id: "2",
      timestamp: 2,
      item: {
        type: "function_call_output",
        call_id: "call_1",
        namespace: "cron",
        name: "list",
        output: "[]",
      },
    },
  ]);

  const activity = messages[0]?.activities?.[0];
  assert.equal(activity?.title, "cron.list");
  assert.equal(activity?.output, "[]");
});

test("projectResponseItemsToEntries renders native shell, web, image, and agent items", () => {
  const messages = projectResponseItemsToEntries([
    {
      id: "1",
      timestamp: 1,
      item: {
        type: "local_shell_call",
        call_id: "shell_1",
        status: "completed",
        action: { command: "pwd" },
      },
    },
    {
      id: "2",
      timestamp: 2,
      item: {
        type: "web_search_call",
        id: "web_1",
        status: "completed",
        action: { query: "weather" },
      },
    },
    {
      id: "3",
      timestamp: 3,
      item: {
        type: "image_generation_call",
        id: "image_1",
        status: "completed",
        revised_prompt: "a blue sky",
        result: "aW1hZ2U=",
      },
    },
    {
      id: "4",
      timestamp: 4,
      item: {
        type: "agent_message",
        author: "worker",
        recipient: "root",
        content: [{ type: "input_text", text: "done" }],
      },
    },
  ]);

  assert.equal(messages.length, 1);
  assert.deepEqual(
    messages[0]?.activities?.map((activity) => activity.title),
    ["local_shell", "web_search", "image_generation"],
  );
  assert.deepEqual(messages[0]?.activities?.[1]?.webAction, {
    type: "search",
    query: "weather",
  });
  assert.deepEqual(messages[0]?.activities?.[2]?.media, [
    { kind: "image", path: "data:image/png;base64,aW1hZ2U=" },
  ]);
  assert.equal(messages[0]?.content, "done");
});

test("projectResponseItemsToEntries enriches browser history with a structured page action", () => {
  const messages = projectResponseItemsToEntries([
    {
      id: "1",
      timestamp: 1,
      item: {
        type: "function_call",
        call_id: "browser_1",
        namespace: "astro_browser",
        name: "snapshot",
        arguments: '{"action":"read"}',
      },
    },
    {
      id: "2",
      timestamp: 2,
      item: {
        type: "function_call_output",
        call_id: "browser_1",
        namespace: "astro_browser",
        name: "snapshot",
        output: JSON.stringify({
          astro_browser: true,
          url: "https://www.bilibili.com/",
          title: "B站",
        }),
      },
    },
  ]);

  assert.deepEqual(messages[0]?.activities?.[0]?.webAction, {
    type: "openPage",
    url: "https://www.bilibili.com/",
  });
  assert.equal(messages[0]?.activities?.[0]?.webPageTitle, "B站");
});

test("normalizeProjectedEntries restores activity media", () => {
  const msgs = normalizeProjectedEntries([
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

test("normalizeProjectedEntries restores interleaved text timeline segments", () => {
  const [message] = normalizeProjectedEntries([
    {
      id: "a1",
      role: "assistant",
      content: "beforeafter",
      segments: [
        { type: "text", id: "txt-1", text: "before", at: 100 },
        { type: "activity", id: "tool-1", at: 200 },
        { type: "text", id: "txt-2", text: "after", at: 300 },
      ],
    },
  ]);

  assert.deepEqual(
    message?.segments?.map((segment) => segment.type),
    ["text", "activity", "text"],
  );
});

test("normalizeProjectedEntries drops invalid media entries", () => {
  const msgs = normalizeProjectedEntries([
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

test("coalesceConsecutiveAssistants merges same-turn assistant bubbles", () => {
  const merged = coalesceConsecutiveAssistants([
    { id: "u1", role: "user", content: "做首歌" },
    {
      id: "a1",
      role: "assistant",
      content: "先生成",
      activities: [
        { id: "c1", kind: "tool", title: "music_gen", status: "done" },
      ],
      segments: [
        { type: "reasoning", id: "r1", text: "t1", at: 1000 },
        { type: "activity", id: "c1", at: 2000 },
      ],
    },
    {
      id: "a2",
      role: "assistant",
      content: "生成成功",
      activities: [
        { id: "c2", kind: "tool", title: "present", status: "done" },
      ],
      segments: [
        { type: "reasoning", id: "r1", text: "t1", at: 1000 },
        { type: "activity", id: "c1", at: 2000 },
        { type: "reasoning", id: "r2", text: "t2", at: 5000 },
        { type: "activity", id: "c2", at: 6000 },
        { type: "surface", id: "surf-1", at: 6100 },
      ],
      uiSurfaces: [
        {
          messageId: "surf-1",
          activityType: "a2ui-surface",
          operations: [],
          status: "active",
        },
      ],
    },
    {
      id: "a3",
      role: "assistant",
      content: "搞定",
      segments: [
        { type: "reasoning", id: "r1", text: "t1", at: 1000 },
        { type: "activity", id: "c1", at: 2000 },
        { type: "reasoning", id: "r2", text: "t2", at: 5000 },
        { type: "activity", id: "c2", at: 6000 },
        { type: "surface", id: "surf-1", at: 6100 },
        { type: "reasoning", id: "r3", text: "t3", at: 7000 },
      ],
    },
  ]);
  assert.equal(merged.length, 2);
  assert.equal(merged[1]!.role, "assistant");
  assert.equal(merged[1]!.content, "先生成\n\n生成成功\n\n搞定");
  assert.equal(merged[1]!.activities?.length, 2);
  assert.equal(merged[1]!.segments?.length, 6);
  assert.equal(merged[1]!.uiSurfaces?.length, 1);
});

test("normalizeProjectedEntries restores durationSec from segment timestamps", () => {
  const msgs = normalizeProjectedEntries([
    {
      id: "a1",
      role: "assistant",
      content: "ok",
      activities: [
        { id: "c1", kind: "tool", title: "music_gen", status: "done" },
      ],
      segments: [
        { type: "reasoning", id: "r1", text: "think", at: 1000 },
        { type: "activity", id: "c1", at: 3500 },
        { type: "reasoning", id: "r2", text: "more", at: 5000 },
      ],
    },
  ]);
  assert.equal(msgs.length, 1);
  const segs = msgs[0]!.segments!;
  assert.equal(segs[0]!.type, "reasoning");
  if (segs[0]!.type === "reasoning") {
    assert.equal(segs[0]!.durationSec, 2.5);
  }
  assert.equal(msgs[0]!.reasoningDurationSec, 2.5);
  assert.equal(msgs[0]!.activities?.[0]?.durationSec, 1.5);
});

test("restored history settles orphaned live activity after restart", () => {
  const msgs = settleRestoredActivities(
    normalizeProjectedEntries([
      {
        id: "a1",
        role: "assistant",
        content: "",
        activities: [
          { id: "c1", kind: "tool", title: "terminal", status: "running" },
        ],
        segments: [{ type: "activity", id: "c1", at: 1_000 }],
      },
    ]),
    4_500,
  );

  assert.equal(msgs[0]!.activities?.[0]?.status, "interrupted");
  assert.equal(msgs[0]!.activities?.[0]?.durationSec, 3.5);
});

test("restored history recognizes extended activity states", () => {
  const [message] = normalizeProjectedEntries([
    {
      id: "a1",
      role: "assistant",
      content: "",
      activities: [
        { id: "waiting", kind: "tool", title: "terminal", status: "waiting" },
        { id: "retrying", kind: "tool", title: "terminal", status: "retrying" },
        { id: "partial", kind: "tool", title: "terminal", status: "partial" },
      ],
    },
  ]);

  assert.deepEqual(
    message?.activities?.map((activity) => activity.status),
    ["waiting", "retrying", "partial"],
  );
});

test("settleRestoredActivities preserves terminal states and existing duration", () => {
  const [message] = settleRestoredActivities(
    [
      {
        id: "a1",
        role: "assistant",
        content: "",
        activities: [
          {
            id: "running",
            kind: "tool",
            title: "terminal",
            status: "running",
            at: 1_000,
            durationSec: 1.25,
          },
          {
            id: "waiting",
            kind: "tool",
            title: "terminal",
            status: "waiting",
            at: 1_500,
          },
          {
            id: "retrying",
            kind: "tool",
            title: "terminal",
            status: "retrying",
            at: 1_750,
          },
          {
            id: "done",
            kind: "tool",
            title: "read",
            status: "done",
            at: 2_000,
          },
        ],
      },
    ],
    9_000,
  );

  assert.equal(message!.activities?.[0]?.status, "interrupted");
  assert.equal(message!.activities?.[0]?.durationSec, 1.25);
  assert.equal(message!.activities?.[1]?.status, "interrupted");
  assert.equal(message!.activities?.[2]?.status, "interrupted");
  assert.equal(message!.activities?.[3]?.status, "done");
});

test("enrichTimelineDurations keeps existing durationSec", () => {
  const out = enrichTimelineDurations([
    { type: "reasoning", id: "r1", text: "a", at: 0, durationSec: 9 },
    { type: "activity", id: "c1", at: 1000 },
  ]);
  assert.equal(out[0]!.type, "reasoning");
  if (out[0]!.type === "reasoning") {
    assert.equal(out[0]!.durationSec, 9);
  }
});

test("normalizeProjectedEntries restores tool batch execution metadata", () => {
  const [message] = normalizeProjectedEntries([
    {
      id: "a1",
      role: "assistant",
      content: "",
      activities: [
        { id: "tool-1", kind: "tool", title: "read_file", status: "done" },
      ],
      segments: [
        {
          type: "activity",
          id: "tool-1",
          at: 10,
          batchId: "batch-1",
          executionMode: "parallel",
        },
      ],
    },
  ]);

  assert.equal(message?.activities?.[0]?.batchId, "batch-1");
  assert.equal(message?.activities?.[0]?.executionMode, "parallel");
});
