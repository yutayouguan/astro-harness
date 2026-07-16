// @ts-nocheck
/**
 * 会话删除与会话变更事件：纯逻辑测试。
 */
import assert from "node:assert/strict";
import { afterEach, before, test } from "node:test";
import {
  deleteManagedSession,
  dispatchSessionsChanged,
  SESSIONS_CHANGED_EVENT,
  subscribeSessionsChanged,
} from "./sessionManagement.ts";

type Listener = (event: Event) => void;

const listeners = new Map<string, Set<Listener>>();

before(() => {
  const g = globalThis as {
    window?: {
      addEventListener: (type: string, listener: Listener) => void;
      removeEventListener: (type: string, listener: Listener) => void;
      dispatchEvent: (event: Event) => boolean;
    };
  };
  g.window = {
    addEventListener(type, listener) {
      const set = listeners.get(type) ?? new Set<Listener>();
      set.add(listener);
      listeners.set(type, set);
    },
    removeEventListener(type, listener) {
      listeners.get(type)?.delete(listener);
    },
    dispatchEvent(event) {
      listeners.get(event.type)?.forEach((listener) => listener(event));
      return true;
    },
  };
});

afterEach(() => {
  listeners.clear();
});

test("dispatchSessionsChanged notifies subscribers", () => {
  let calls = 0;
  const unsubscribe = subscribeSessionsChanged(() => {
    calls += 1;
  });

  dispatchSessionsChanged();
  unsubscribe();
  dispatchSessionsChanged();

  assert.equal(calls, 1);
});

test("deleting active session clears local chat after backend success", async () => {
  const calls: string[] = [];

  await deleteManagedSession(
    "s1",
    "s1",
    async () => {
      calls.push("invoke");
    },
    async () => {
      calls.push("clear");
    },
  );

  assert.deepEqual(calls, ["invoke", "clear"]);
});

test("failed delete does not clear active chat", async () => {
  let cleared = false;

  await assert.rejects(() =>
    deleteManagedSession(
      "s1",
      "s1",
      async () => {
        throw new Error("db");
      },
      async () => {
        cleared = true;
      },
    ),
  );

  assert.equal(cleared, false);
});

test("deleting non-active session does not clear local chat", async () => {
  const calls: string[] = [];

  await deleteManagedSession(
    "s1",
    "s2",
    async () => {
      calls.push("invoke");
    },
    async () => {
      calls.push("clear");
    },
  );

  assert.deepEqual(calls, ["invoke"]);
});

test("session event name stays stable", () => {
  assert.equal(SESSIONS_CHANGED_EVENT, "astro:sessions-changed");
});
