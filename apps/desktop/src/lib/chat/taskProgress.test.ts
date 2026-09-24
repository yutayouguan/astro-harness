import assert from "node:assert/strict";
import test from "node:test";
import type { ConversationEntry } from "../../types.ts";
import {
  displayFileName,
  extractFileChangeSummary,
  extractLatestTodoPlan,
  extractTurnFileChangeSummary,
  isTodoActivity,
  isTodoOnlyActivityMessage,
  resolveFileReviewPath,
} from "./taskProgress.ts";

const patch = `*** Begin Patch
*** Update File: src/a.ts
@@
-old
+new
+extra
*** Add File: src/b.ts
+one
+two
*** Delete File: src/c.ts
-gone
-also gone
*** End Patch`;

const messages: ConversationEntry[] = [
  { id: "u-1", role: "user", content: "实现状态条" },
  {
    id: "a-1",
    role: "assistant",
    content: "",
    activities: [
      {
        id: "todo-1",
        kind: "tool",
        title: "todo",
        input: JSON.stringify({
          action: "create",
          title: "实现任务状态",
          items: [
            { text: "定位组件", done: true },
            { text: "实现交互", done: false },
          ],
        }),
        status: "done",
      },
      {
        id: "patch-1",
        kind: "tool",
        title: "apply_patch",
        input: JSON.stringify(patch),
        status: "done",
      },
    ],
  },
];

test("extracts the latest todo plan", () => {
  assert.deepEqual(extractLatestTodoPlan(messages), {
    title: "实现任务状态",
    planId: "",
    items: [
      { text: "定位组件", done: true },
      { text: "实现交互", done: false },
    ],
  });
});

test("todo activities remain available to the progress bar but can be omitted from answers", () => {
  const todoActivity = messages[1]!.activities![0]!;
  assert.equal(isTodoActivity(todoActivity), true);
  assert.equal(
    isTodoOnlyActivityMessage({
      id: "todo-only",
      role: "assistant",
      content: "",
      activities: [todoActivity],
    }),
    true,
  );
  assert.equal(isTodoOnlyActivityMessage(messages[1]!), false);
});

test("aggregates per-file line changes from apply_patch", () => {
  assert.deepEqual(extractFileChangeSummary(messages), {
    additions: 4,
    deletions: 3,
    items: [
      { path: "src/a.ts", additions: 2, deletions: 1 },
      { path: "src/b.ts", additions: 2, deletions: 0 },
      { path: "src/c.ts", additions: 0, deletions: 2 },
    ],
  });
});

test("a newly created plan resets prior file records", () => {
  const next: ConversationEntry[] = [
    ...messages,
    {
      id: "a-2",
      role: "assistant",
      content: "",
      activities: [
        {
          id: "todo-2",
          kind: "tool",
          title: "default_api:todo",
          input: JSON.stringify({ items: ["新任务"] }),
          status: "done",
        },
        {
          id: "write-1",
          kind: "tool",
          title: "write_file",
          input: JSON.stringify({
            path: "src/new.ts",
            content: "one\ntwo\n",
          }),
          status: "done",
        },
      ],
    },
  ];
  assert.deepEqual(extractFileChangeSummary(next), {
    additions: 2,
    deletions: 0,
    items: [{ path: "src/new.ts", additions: 2, deletions: 0 }],
  });
});

test("uses the basename for compact file rows", () => {
  assert.equal(displayFileName("apps/desktop/src/App.tsx"), "App.tsx");
  assert.equal(displayFileName("src\\main.rs"), "main.rs");
});

test("accepts structured file_change records from the thread protocol", () => {
  const structured: ConversationEntry[] = [
    ...messages.slice(0, 1),
    {
      id: "a-file-change",
      role: "assistant",
      content: "",
      activities: [
        messages[1]!.activities![0]!,
        {
          id: "change-1",
          kind: "tool",
          title: "file_change",
          input: JSON.stringify({
            files: [{ path: "src/protocol.rs", additions: 9, deletions: 1 }],
          }),
          status: "done",
        },
      ],
    },
  ];
  assert.deepEqual(extractFileChangeSummary(structured), {
    additions: 9,
    deletions: 1,
    items: [{ path: "src/protocol.rs", additions: 9, deletions: 1 }],
  });
});

test("aggregates exact file snapshots within one assistant turn", () => {
  const message: ConversationEntry = {
    id: "a-turn",
    role: "assistant",
    content: "done",
    activities: [
      {
        id: "patch-1",
        kind: "tool",
        title: "apply_patch",
        fileChanges: [
          {
            root: "/repo",
            path: "src/a.ts",
            kind: "update",
            before_content: "old\n",
            after_content: "middle\n",
            additions: 1,
            deletions: 1,
            reversible: true,
          },
        ],
      },
      {
        id: "patch-2",
        kind: "tool",
        title: "apply_patch",
        fileChanges: [
          {
            root: "/repo",
            path: "src/a.ts",
            kind: "update",
            before_content: "middle\n",
            after_content: "new\n",
            additions: 1,
            deletions: 1,
            reversible: true,
          },
        ],
      },
    ],
  };
  assert.deepEqual(extractTurnFileChangeSummary(message), {
    additions: 1,
    deletions: 1,
    items: [
      {
        root: "/repo",
        path: "src/a.ts",
        sourcePath: "src/a.ts",
        kind: "update",
        beforeContent: "old\n",
        afterContent: "new\n",
        additions: 1,
        deletions: 1,
        reversible: true,
      },
    ],
  });
});

test("todo file changes keep recorded snapshots for delete + re-add of one path", () => {
  const workspace = "/Users/me/.astro/workspace";
  const target = "../skills/aihot/SKILL.md";
  const summary = extractFileChangeSummary([
    { id: "u-1", role: "user", content: "写入 aihot skill" },
    {
      id: "a-1",
      role: "assistant",
      content: "",
      activities: [
        {
          id: "todo-1",
          kind: "tool",
          title: "todo",
          input: JSON.stringify({
            action: "create",
            items: [{ text: "写入 SKILL.md", done: true }],
          }),
          status: "done",
        },
        {
          id: "patch-1",
          kind: "tool",
          title: "apply_patch",
          input: JSON.stringify(
            "*** Begin Patch\n*** Delete File: ../skills/aihot/SKILL.md\n*** Add File: ../skills/aihot/SKILL.md\n+new\n*** End Patch",
          ),
          status: "done",
          fileChanges: [
            {
              root: workspace,
              path: target,
              kind: "delete",
              before_content: "old\n",
              additions: 0,
              deletions: 1,
              reversible: true,
            },
            {
              root: workspace,
              path: target,
              kind: "add",
              after_content: "new\n",
              additions: 1,
              deletions: 0,
              reversible: true,
            },
          ],
        },
      ],
    },
  ]);

  assert.deepEqual(summary, {
    additions: 1,
    deletions: 1,
    items: [
      {
        root: workspace,
        path: target,
        sourcePath: target,
        kind: "add",
        beforeContent: "old\n",
        afterContent: "new\n",
        additions: 1,
        deletions: 1,
        reversible: true,
      },
    ],
  });
});

test("resolves review paths against the recorded workspace root", () => {
  assert.equal(
    resolveFileReviewPath({
      root: "/Users/me/.astro/workspace",
      path: "../skills/aihot/SKILL.md",
    }),
    "/Users/me/.astro/skills/aihot/SKILL.md",
  );
  assert.equal(
    resolveFileReviewPath({ root: "/repo/apps", path: "./src/../src/main.rs" }),
    "/repo/apps/src/main.rs",
  );
  assert.equal(
    resolveFileReviewPath({ root: "/repo/", path: "/repo/src/main.rs" }),
    "/repo/src/main.rs",
  );
  assert.equal(
    resolveFileReviewPath({ path: "../skills/aihot/SKILL.md" }),
    "../skills/aihot/SKILL.md",
  );
});
