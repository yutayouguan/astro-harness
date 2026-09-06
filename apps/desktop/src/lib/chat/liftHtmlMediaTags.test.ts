import assert from "node:assert/strict";
import test from "node:test";
import { liftHtmlMediaTags } from "./liftHtmlMediaTags.ts";

test("lifts audio and video tags to markdown images", () => {
  const out = liftHtmlMediaTags(
    '听：<audio src="generated/audio/a.mp3" controls></audio>\n看：<video src="generated/videos/b.mp4" controls />',
  );
  assert.match(out, /!\[audio\]\(generated\/audio\/a\.mp3\)/);
  assert.match(out, /!\[video\]\(generated\/videos\/b\.mp4\)/);
  assert.doesNotMatch(out, /<audio/i);
  assert.doesNotMatch(out, /<video/i);
});

test("lifts video with nested source", () => {
  const out = liftHtmlMediaTags(
    '<video controls><source src="clip.webm" type="video/webm"></video>',
  );
  assert.match(out, /!\[video\]\(clip\.webm\)/);
});

test("skips fenced code blocks", () => {
  const src =
    '正文<audio src="a.mp3"></audio>\n```html\n<audio src="keep.mp3"></audio>\n```\n';
  const out = liftHtmlMediaTags(src);
  assert.match(out, /!\[audio\]\(a\.mp3\)/);
  assert.match(out, /<audio src="keep\.mp3"><\/audio>/);
});
