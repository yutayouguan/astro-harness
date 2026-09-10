import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");
test("pet manager separates library creation and global settings using keyboard tabs", () => {
  const panel = read("../../components/settings/DesktopPetPanel.tsx");
  assert.match(panel, /useState\("library"\)/);
  for (const tab of ["library", "create", "general"])
    assert.ok(panel.includes(`pet-${tab}-panel`));
  assert.match(panel, /<SegmentedTabs/);
  assert.doesNotMatch(panel, /desktop-pet-hero|conceptImage|desktop-pet-grid/);
  const library = read("../../components/settings/PetLibraryPanel.tsx");
  assert.match(library, /onClick=\{\(\) => setSelected\(item.id\)\}/);
  assert.match(library, /apply_library_pet/);
  assert.match(library, /pet-detail-actions/);
  assert.match(library, /confirmSceneCount: deleting/);
  assert.match(library, /active=\{false\}/);
});
test("scene editor scopes drafts locally and exposes inheritance and explicit apply", () => {
  const source = read("../../components/settings/PetSceneLibrary.tsx");
  assert.match(source, /scenesForPet\(scenes, pet.id\)/);
  assert.match(source, /set_preferences/);
  assert.match(source, /scene.wallpaperPath \? "all" : "pet"/);
  assert.match(source, /cancel_pet_generation/);
  assert.match(source, /<PetMoreMenu/);
  const menu = read("../../components/settings/PetMoreMenu.tsx");
  assert.match(menu, /Escape/);
  assert.match(menu, /pointerdown/);
  assert.match(menu, /aria-expanded/);
});
