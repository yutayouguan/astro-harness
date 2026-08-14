import assert from "node:assert/strict";
import test from "node:test";
import { displayTitleFromMediaPath } from "./displayTitleFromMediaPath.ts";

test("strips timestamp-uuid suffix for chinese titles", () => {
  assert.equal(
    displayTitleFromMediaPath(
      "generated/audio/采菌子歌-20260717-194651-d81f0b2b.mp3",
    ),
    "采菌子歌",
  );
});

test("maps legacy prefixes to chinese kind labels", () => {
  assert.equal(
    displayTitleFromMediaPath("generated/audio/music-20260717-194651-d81f0b2b.mp3"),
    "音乐",
  );
  assert.equal(
    displayTitleFromMediaPath("generated/images/img-20260717-110609-5fb9089c.jpg"),
    "图片",
  );
});

test("keeps plain stem when no stamp", () => {
  assert.equal(displayTitleFromMediaPath("generated/videos/demo.mp4"), "demo");
});
