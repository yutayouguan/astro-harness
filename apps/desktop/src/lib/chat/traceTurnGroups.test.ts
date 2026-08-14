import { describe, expect, it } from "vitest";
import {
  groupEventsByUserTurns,
  groupEventsForTraceDisplay,
  traceSessionTitle,
  turnGroupTitle,
} from "./traceTurnGroups";

describe("groupEventsByUserTurns", () => {
  it("splits on each user message and keeps following spans", () => {
    const groups = groupEventsByUserTurns([
      { kind: "user", output: "天气如何", total_tokens: 0, cost_usd: 0 },
      {
        kind: "tool",
        name: "web_search",
        turn_id: "t1",
        total_tokens: 0,
        cost_usd: 0,
      },
      {
        kind: "llm",
        name: "gpt",
        turn_id: "t1",
        total_tokens: 12,
        cost_usd: 0.01,
      },
      { kind: "user", output: "再细一点", total_tokens: 0, cost_usd: 0 },
      {
        kind: "llm",
        name: "gpt",
        turn_id: "t2",
        total_tokens: 8,
        cost_usd: 0,
      },
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0].events.map((e) => e.kind)).toEqual(["user", "tool", "llm"]);
    expect(groups[0].turn_id).toBe("t1");
    expect(groups[0].tokens).toBe(12);
    expect(groups[1].events.map((e) => e.kind)).toEqual(["user", "llm"]);
    expect(groups[1].turn_id).toBe("t2");
  });
});

describe("turnGroupTitle", () => {
  it("prefers user question text", () => {
    expect(
      turnGroupTitle(
        [
          { kind: "user", output: "帮我查一下北京天气", total_tokens: 0, cost_usd: 0 },
          { kind: "llm", name: "gpt", turn_id: "abcd1234-xxxx", total_tokens: 1, cost_usd: 0 },
        ],
        "abcd1234-xxxx",
        "未标注",
        "回合",
      ),
    ).toBe("帮我查一下北京天气");
  });

  it("falls back to short turn id", () => {
    expect(
      turnGroupTitle(
        [{ kind: "llm", name: "assistant", turn_id: "abcd1234-xxxx", total_tokens: 1, cost_usd: 0 }],
        "abcd1234-xxxx",
        "未标注",
        "回合",
      ),
    ).toBe("回合 abcd1234…");
  });
});

describe("groupEventsForTraceDisplay", () => {
  it("falls back to turn_id groups when there is no user span", () => {
    const groups = groupEventsForTraceDisplay([
      { kind: "llm", turn_id: "t1", total_tokens: 3, cost_usd: 0 },
      { kind: "tool", turn_id: "t1", total_tokens: 0, cost_usd: 0 },
      { kind: "llm", turn_id: "t2", total_tokens: 5, cost_usd: 0 },
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0].turn_id).toBe("t1");
    expect(groups[1].turn_id).toBe("t2");
  });
});

describe("traceSessionTitle", () => {
  it("uses title when present", () => {
    expect(traceSessionTitle("查天气", "未命名会话")).toBe("查天气");
  });

  it("falls back to an unnamed label instead of UUID", () => {
    expect(traceSessionTitle("", "未命名会话")).toBe("未命名会话");
    expect(traceSessionTitle(undefined, "Unnamed session")).toBe(
      "Unnamed session",
    );
  });
});
