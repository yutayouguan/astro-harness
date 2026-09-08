import assert from "node:assert/strict";
import test from "node:test";
import { loadPetAtlas, type AtlasImage } from "./desktopPetAtlas.ts";

test("only a decoded v2 atlas reaches the player", () => {
  const image: AtlasImage = {
    src: "",
    naturalWidth: 1536,
    naturalHeight: 2288,
    onload: null,
    onerror: null,
  };
  let loaded = 0;
  let failed = 0;
  const stop = loadPetAtlas({
    src: "atlas.webp",
    createImage: () => image,
    loaded: () => loaded++,
    failed: () => failed++,
  });
  image.onload?.(new Event("load"));
  assert.equal(loaded, 1);
  image.naturalHeight = 1872;
  image.onload?.(new Event("load"));
  assert.equal(failed, 1);
  image.onerror?.(new Event("error"));
  assert.equal(failed, 2);
  stop();
});

test("cancelled old atlas callbacks cannot paint over the replacement", () => {
  const image: AtlasImage = {
    src: "",
    naturalWidth: 1536,
    naturalHeight: 2288,
    onload: null,
    onerror: null,
  };
  let updates = 0;
  const stop = loadPetAtlas({
    src: "old.webp",
    createImage: () => image,
    loaded: () => updates++,
    failed: () => updates++,
  });
  const lateLoad = image.onload;
  const lateError = image.onerror;
  stop();
  lateLoad?.(new Event("load"));
  lateError?.(new Event("error"));
  assert.equal(updates, 0);
  assert.equal(image.onload, null);
});
