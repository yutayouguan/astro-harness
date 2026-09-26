import type { Meta, StoryObj } from "@storybook/react-vite";
import { useMemo, useState } from "react";
import MsgActivityGroup from "../components/chat/MsgActivityGroup";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import type { ChatActivity } from "../types";

type Row = { key: string; activities: ChatActivity[] };

function activity(row: string, index: number): ChatActivity {
  return {
    id: `${row}-${index}`,
    kind: "tool",
    title: index % 2 === 0 ? "terminal" : "read_file",
    input: `{"cmd":"echo ${row}-${index}"}`,
    output: `done ${row}-${index}`,
    status: "done",
    at: 1_700_000_000_000 + index * 1_000,
    durationSec: 1.2,
  };
}

declare global {
  interface Window {
    /** 活动行渲染期间读取 props 的次数：用于断言「props 未变则不重渲染」。 */
    __propReads?: number;
  }
}

/**
 * 流式 flush 只替换当前消息对象：历史活动行 props 不变时必须跳过重渲染。
 * 这里给 activity 的字段挂 getter 统计读取次数，供 visual-tests 断言
 * 「flush 后历史行没有再次读取 props」。
 */
function probedActivity(row: string, index: number): ChatActivity {
  const base = activity(row, index);
  return new Proxy(base, {
    get(target, key) {
      // status 由分组自身在渲染时统计，title/output 由活动卡读取。
      if (key === "status" || key === "title" || key === "output") {
        window.__propReads = (window.__propReads ?? 0) + 1;
      }
      return Reflect.get(target, key as keyof ChatActivity);
    },
  });
}

function ChatActivityRowsSample() {
  const [tick, setTick] = useState(0);
  const rows = useMemo<Row[]>(
    () => [
      {
        key: "row-a",
        activities: [probedActivity("row-a", 0), probedActivity("row-a", 1)],
      },
      {
        key: "row-b",
        activities: [probedActivity("row-b", 0), probedActivity("row-b", 1)],
      },
    ],
    [],
  );
  // 只在首次挂载时清零：重渲染不能把它归零，否则断言失去意义。
  if (window.__propReads == null) window.__propReads = 0;

  return (
    <div className="app-shell" data-tone="blue" style={{ padding: 16 }}>
      <button
        type="button"
        className="ui-button"
        onClick={() => setTick((value) => value + 1)}
      >
        flush
      </button>
      <span data-testid="tick">{tick}</span>
      <div className="message-list">
        {rows.map((row) => (
          <MsgActivityGroup
            key={row.key}
            activities={row.activities}
            defaultOpen={false}
            showTimestamp={false}
          />
        ))}
      </div>
    </div>
  );
}

const meta = {
  title: "Chat/Activity Rows",
  component: ChatActivityRowsSample,
  parameters: { layout: "fullscreen" },
  decorators: [
    (Story) => (
      <LocaleProvider>
        <MorphiconProvider>
          <Story />
        </MorphiconProvider>
      </LocaleProvider>
    ),
  ],
} satisfies Meta<typeof ChatActivityRowsSample>;
export default meta;
type Story = StoryObj<typeof meta>;

export const StreamingFlush: Story = {};
