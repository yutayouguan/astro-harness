import assert from "node:assert/strict";
import test from "node:test";
import { applyUiStyleTokenDiff } from "./uiStyleTokenDiff.ts";

test("wallpaper-only revisions make no theme token writes", () => {
  const values = new Map([["--color-accent", "#7855cc"]]);
  const writes: string[] = [];
  const style = {
    getPropertyValue: (key: string) => values.get(key) ?? "",
    setProperty: (key: string, value: string) => {
      writes.push(key);
      values.set(key, value);
    },
    removeProperty: (key: string) => {
      writes.push(key);
      values.delete(key);
      return "";
    },
  };
  const keys = applyUiStyleTokenDiff(style, ["--color-accent"], {
    "--color-accent": "#7855cc",
  });
  assert.deepEqual(writes, []);
  applyUiStyleTokenDiff(style, keys, {
    "--color-accent": "#ff8844",
    "--color-text": "#eee",
  });
  assert.deepEqual(writes, ["--color-accent", "--color-text"]);
  writes.length = 0;
  applyUiStyleTokenDiff(style, ["--color-accent", "--color-text"], {
    "--color-accent": "#ff8844",
  });
  assert.deepEqual(writes, ["--color-text"]);
});
