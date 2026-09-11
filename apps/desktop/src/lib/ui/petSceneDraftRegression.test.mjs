import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("applying pet defaults does not close an unrelated scene-name draft", () => {
  const panel = read("../../components/settings/PetLibraryPanel.tsx");
  const run = panel.slice(
    panel.indexOf("async function run("),
    panel.indexOf("return (", panel.indexOf("async function run(")),
  );
  assert.match(
    run,
    /command === "edit_pet_library"[\s\S]*?requestAction === "rename"[\s\S]*?requestAction === "add_scene"[\s\S]*?setEditor\(null\)/,
  );
  assert.match(
    run,
    /command === "edit_pet_library" && requestAction === "delete"/,
  );
  assert.doesNotMatch(
    run,
    /await mutate\(command, args\);\s*setEditor\(null\)/,
  );
});

test("scene examples use an animal-free environment instead of the UI concept", () => {
  const story = read("../../stories/PetSceneStudio.stories.tsx");
  assert.match(story, /roomWallpaper from "\.\/assets\/pet-room.jpeg"/);
  assert.match(story, /wallpaperPath: sceneWallpaper/);
  assert.doesNotMatch(story, /wallpaperPath: photo/);
});
