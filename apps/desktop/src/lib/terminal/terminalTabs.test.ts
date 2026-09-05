import assert from "node:assert/strict";
import { test } from "node:test";

import {
  MAX_TERMINAL_TABS,
  defaultTerminalTabs,
  readTerminalTabLayout,
} from "./terminalTabs.ts";

test("new user terminals honor the configured execution mode", () => {
  const layout = defaultTerminalTabs("/workspace", "User", "AI", "project");

  assert.equal(layout.tabs.length, 2);
  assert.equal(layout.tabs[0].executionMode, "project");
  assert.equal(layout.tabs[0].agentDefault, false);
  assert.equal(layout.tabs[1].executionMode, "project");
  assert.equal(layout.tabs[1].agentDefault, true);
});

test("stored layouts are bounded and repaired to one project-scoped AI terminal", () => {
  const originalWindow = globalThis.window;
  const storedTabs = Array.from(
    { length: MAX_TERMINAL_TABS + 3 },
    (_, index) => ({
      clientId: index === 1 ? "tab-0" : `tab-${index}`,
      title: `Stored ${index}`,
      cwd: "/untrusted/path",
      executionMode: index === 2 ? "project" : "system",
      agentDefault: index === 0 || index === 2,
    }),
  );
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      localStorage: {
        getItem: () =>
          JSON.stringify({ activeClientId: "missing", tabs: storedTabs }),
      },
    },
  });

  try {
    const layout = readTerminalTabLayout("project", "/workspace", "User", "AI");
    assert.equal(layout.tabs.length, MAX_TERMINAL_TABS);
    assert.equal(
      new Set(layout.tabs.map((tab) => tab.clientId)).size,
      layout.tabs.length,
    );
    assert.equal(layout.tabs.filter((tab) => tab.agentDefault).length, 1);
    assert.equal(
      layout.tabs.find((tab) => tab.agentDefault)?.executionMode,
      "project",
    );
    assert.ok(layout.tabs.every((tab) => tab.cwd === "/workspace"));
    assert.equal(layout.activeClientId, layout.tabs[0].clientId);
  } finally {
    if (originalWindow === undefined)
      delete (globalThis as { window?: Window }).window;
    else globalThis.window = originalWindow;
  }
});
