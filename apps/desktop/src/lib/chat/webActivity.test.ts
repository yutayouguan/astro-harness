import assert from "node:assert/strict";
import test from "node:test";
import {
  deriveChatWebActivity,
  normalizeChatWebAction,
} from "./webActivity.ts";

test("projects Codex-style search, open-page, and find-in-page actions", () => {
  assert.deepEqual(
    deriveChatWebActivity({
      name: "web_search",
      input: '{"type":"search","query":"Responses API"}',
    }),
    { action: { type: "search", query: "Responses API" } },
  );
  assert.deepEqual(
    deriveChatWebActivity({
      name: "browser.open",
      input: '{"url":"https://example.com/start"}',
      output: '{"url":"https://example.com/final","title":"Example page"}',
    }),
    {
      action: { type: "openPage", url: "https://example.com/final" },
      pageTitle: "Example page",
    },
  );
  assert.deepEqual(
    deriveChatWebActivity({
      name: "web.find_in_page",
      input:
        '{"type":"find_in_page","url":"https://example.com","pattern":"pricing"}',
    }),
    {
      action: {
        type: "findInPage",
        url: "https://example.com",
        pattern: "pricing",
      },
    },
  );
});

test("projects browser snapshot metadata and ignores JSON read from a file", () => {
  assert.deepEqual(
    deriveChatWebActivity({
      name: "browser.snapshot",
      input: '{"action":"read"}',
      output: JSON.stringify({
        astro_browser: true,
        active_tab_id: "tab-b",
        tabs: [
          { id: "tab-a", url: "https://example.com", title: "Example" },
          {
            id: "tab-b",
            url: "https://www.bilibili.com/",
            title: "B站",
            active: true,
          },
        ],
      }),
    }),
    {
      action: { type: "openPage", url: "https://www.bilibili.com/" },
      pageTitle: "B站",
    },
  );
  assert.equal(
    deriveChatWebActivity({
      name: "read_file",
      input: '{"path":"site.json"}',
      output: '{"url":"https://example.com","title":"Example"}',
    }),
    undefined,
  );
});

test("rejects non-http page URLs", () => {
  assert.deepEqual(
    deriveChatWebActivity({
      name: "browser.open",
      input: '{"url":"javascript:alert(1)"}',
    }),
    { action: { type: "other" } },
  );
  assert.deepEqual(
    deriveChatWebActivity({
      name: "browser.click",
      input: '{"selector":"#submit"}',
      output: '{"url":"https://example.com/after-click"}',
    }),
    { action: { type: "other" } },
  );
});

test("normalizes persisted actions without widening their shape", () => {
  assert.deepEqual(
    normalizeChatWebAction({
      type: "findInPage",
      url: "https://example.com",
      pattern: "docs",
      ignored: true,
    }),
    { type: "findInPage", url: "https://example.com", pattern: "docs" },
  );
  assert.equal(normalizeChatWebAction({ type: "unknown" }), undefined);
});
