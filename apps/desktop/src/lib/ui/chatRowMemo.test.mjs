import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const chatDir = new URL("../../components/chat/", import.meta.url);
const timelineUrl = new URL("../chat/chatTimeline.ts", import.meta.url);

const MEMOIZED_ROWS = [
  "MsgActivity",
  "MsgActivityGroup",
  "MsgReasoning",
  "MsgCitations",
];

test("streaming flushes skip unchanged chat rows", async () => {
  // 历史行只有在 props 引用不变时才能被 memo 跳过；流式更新必须走不可变路径，
  // 否则原地改对象会让 memo 直接吞掉更新。
  const timeline = await readFile(timelineUrl, "utf8");
  assert.match(
    timeline,
    /const activities = \[\.\.\.\(base\.activities \?\? \[\]\)\];/,
  );
  assert.match(timeline, /activities\[existing\] = \{/);
  assert.match(
    timeline,
    /activities\.push\(\{ \.\.\.activity, at, durationSec \}\)/,
  );
  assert.doesNotMatch(timeline, /m\.activities\.(push|splice|sort)\(/);

  for (const name of MEMOIZED_ROWS) {
    const source = await readFile(new URL(`${name}.tsx`, chatDir), "utf8");
    assert.match(source, /import \{[^}]*\bmemo\b[^}]*\} from "react";/);
    assert.match(source, new RegExp(`export default memo\\(${name}Impl\\);`));
  }
});
