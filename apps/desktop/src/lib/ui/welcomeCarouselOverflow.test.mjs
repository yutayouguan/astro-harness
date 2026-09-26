import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { readChatCss } from "./chatCssSource.mjs";

const styles = await readChatCss();

test("welcome marquee keeps vertical hover clearance inside clipped rows", () => {
  assert.match(
    styles,
    /\.chat-welcome-marquee-wrap\s*\{[\s\S]*?gap:\s*0;[\s\S]*?padding:\s*4px 0;/,
  );
  assert.match(
    styles,
    /\.chat-welcome-marquee\s*\{[\s\S]*?overflow:\s*hidden;[\s\S]*?padding-block:\s*10px 14px;/,
  );
});
