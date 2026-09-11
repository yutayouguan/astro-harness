import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("pet detail creation belongs to the scenes panel instead of shifting the tabs", () => {
  const panel = read("../../components/settings/PetLibraryPanel.tsx");
  const scenePanel = panel.indexOf('id="pet-detail-scenes"');
  const form = panel.indexOf('editor?.action === "add_scene" && editorForm');
  assert.ok(scenePanel > 0 && form > scenePanel);
  assert.match(panel, /pet-detail-name-editor/);
  assert.match(panel, /e\.key === "Escape"/);
  assert.match(panel, /!e\.nativeEvent\.isComposing/);
  assert.match(panel, /sceneCreateButton\.current\?\.focus/);
});

test("empty state waits for the scene read and disappears while creating", () => {
  const scenes = read("../../components/settings/PetSceneLibrary.tsx");
  assert.match(scenes, /setLoading\(true\)/);
  assert.match(scenes, /setLoadError\(""\)/);
  assert.match(scenes, /setLoadError\(String\(e\)\)/);
  assert.match(scenes, /finally\(/);
  assert.match(scenes, /!loading &&[\s\S]*?!creating/);
  assert.match(scenes, /还没有专属场景/);
});

test("preference editor has labelled switches, accurate dirty state and a safe submit", () => {
  const editor = read("../../components/settings/PetPreferencesEditor.tsx");
  assert.match(editor, /role="switch"/);
  assert.match(editor, /aria-describedby/);
  assert.match(editor, /JSON\.stringify\(draft\) !== signature/);
  assert.match(editor, /unavailable \|\| draft\.behavior\.quietMode/);
  assert.match(editor, /有未保存的更改/);
  assert.match(editor, /if \(unavailable \|\| !dirty\) return/);
});

test("detail styling is isolated from creation and global settings", () => {
  const css = read("../../styles/features/pet-detail.css");
  assert.match(css, /\.pet-library\.pet-detail-view/);
  assert.match(css, /@container pet-detail/);
  assert.doesNotMatch(css, /\.pet-create|\.pet-general|\.desktop-pet-hero/);
  assert.match(css, /--seg-shell-bg: transparent/);
  assert.match(css, /\.pet-detail-actions > \.pet-more\s*\{\s*margin-left: 0/);
});
