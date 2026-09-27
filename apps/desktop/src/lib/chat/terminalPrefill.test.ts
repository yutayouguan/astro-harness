import assert from "node:assert/strict";
import test from "node:test";
import {
  TERMINAL_PREFILL_EVENT,
  openCommandInTerminal,
  type TerminalPrefillRequest,
} from "./terminalPrefill.ts";

function withStubbedWindow(run: (events: TerminalPrefillRequest[]) => void) {
  const events: TerminalPrefillRequest[] = [];
  const original = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      dispatchEvent: (event: CustomEvent<TerminalPrefillRequest>) => {
        events.push(event.detail);
        return true;
      },
    },
  });
  try {
    run(events);
  } finally {
    if (original) Object.defineProperty(globalThis, "window", original);
    else delete (globalThis as { window?: unknown }).window;
  }
}

test("openCommandInTerminal dispatches a prefill request with a fresh token", () => {
  withStubbedWindow((events) => {
    assert.equal(openCommandInTerminal("  cat README.md  "), true);
    assert.equal(openCommandInTerminal("cat README.md"), true);

    assert.equal(events.length, 2);
    assert.equal(events[0]?.command, "cat README.md");
    // 同一个命令连点两次也要能再开：token 必须递增。
    assert.ok((events[1]?.token ?? 0) > (events[0]?.token ?? 0));
    assert.equal(TERMINAL_PREFILL_EVENT, "astro:terminal-prefill");
  });
});

test("openCommandInTerminal ignores blank commands", () => {
  withStubbedWindow((events) => {
    assert.equal(openCommandInTerminal("   \n"), false);
    assert.equal(events.length, 0);
  });
});
