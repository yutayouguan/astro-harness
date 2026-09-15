import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("creation separates static generation and import while retaining cancellation", () => {
  const source = read("../../components/settings/PetCreatePanel.tsx");
  assert.match(source, /className="prefs-card pet-import-card"/);
  assert.match(source, /className="pet-create-submit"/);
  assert.match(source, /hidden=\{!withWallpaper\}/);
  assert.match(source, /照片仅生成静态形象；动画可用内置奶糖或导入宠物包/);
  assert.match(source, /cancel_pet_generation/);
  assert.match(source, /disabled=\{!state.sourcePath \|\| busy != null\}/);
  assert.doesNotMatch(source, /<details|<summary/);
});

test("general settings group local help and reuse accessible switch styling", () => {
  const source = read("../../components/settings/DesktopPetPanel.tsx");
  assert.match(source, /显示与联动/);
  assert.match(source, /免打扰/);
  assert.match(source, /pet-general-recovery/);
  for (const command of [
    "set_desktop_pet_always_on_top",
    "set_pet_scene_follow_wallpaper",
    "configure_desktop_pet_preferences",
    "reset_desktop_pet_position",
    "resume_desktop_pet",
  ])
    assert.ok(source.includes(command));
  const toggle = read("../../components/settings/PetSettingSwitch.tsx");
  assert.match(toggle, /role="switch"/);
  assert.match(toggle, /aria-checked=\{checked\}/);
  assert.match(toggle, /aria-describedby/);
  assert.match(toggle, /htmlFor=\{id\}/);
  assert.match(toggle, /disabled=\{disabled\}/);
  assert.match(toggle, /prefs-switch-thumb/);
});
