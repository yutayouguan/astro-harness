import assert from "node:assert/strict";
import test from "node:test";
import { parseGeneratedMedia } from "./parseGeneratedMedia.ts";

test("parses labeled image_gen / video_gen / tts output", () => {
  const text = [
    "图片已生成：/Users/a/.astro/ws/generated/img-1.png",
    "provider=google",
    "model=gemini-3.1-flash-image",
  ].join("\n");
  assert.deepEqual(parseGeneratedMedia(text), [
    { kind: "image", path: "/Users/a/.astro/ws/generated/img-1.png" },
  ]);

  assert.deepEqual(
    parseGeneratedMedia(
      "视频已生成：/tmp/vid-1.mp4\nprovider=google\nmodel=veo\noperation_id=op1",
    ),
    [{ kind: "video", path: "/tmp/vid-1.mp4" }],
  );

  assert.deepEqual(
    parseGeneratedMedia("语音已生成：/tmp/tts-1.wav\nprovider=google\nmodel=x"),
    [{ kind: "audio", path: "/tmp/tts-1.wav" }],
  );
});

test("dedupes and falls back to bare media paths", () => {
  const text =
    "saved at /Users/a/generated/img-2.webp and also /Users/a/generated/clip.mp4";
  const items = parseGeneratedMedia(text);
  assert.equal(items.length, 2);
  assert.equal(items[0]?.kind, "image");
  assert.equal(items[1]?.kind, "video");
});

test("empty / non-media returns []", () => {
  assert.deepEqual(parseGeneratedMedia(""), []);
  assert.deepEqual(parseGeneratedMedia("ok done"), []);
  assert.deepEqual(parseGeneratedMedia(null), []);
});
