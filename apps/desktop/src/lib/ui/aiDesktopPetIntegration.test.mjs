import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("chat-driven desktop pet chain is wired from bundled skill to native window", () => {
  const skill = read(
    "../../../../../crates/agent-skills/bundled/desktop-pet-creator/SKILL.md",
  );
  const seed = read("../../../../../crates/agent-skills/src/seed.rs");
  const tool = read(
    "../../../../../crates/agent-tools/src/builtin/desktop_pet.rs",
  );
  const imageTool = read(
    "../../../../../crates/agent-tools/src/builtin/media/image_gen.rs",
  );
  const toolModules = read(
    "../../../../../crates/agent-tools/src/builtin/mod.rs",
  );
  const toolGates = read(
    "../../../../../crates/agent-home/src/config/tools_enabled.rs",
  );
  const desktop = read("../../../src-tauri/src/commands/ui/desktop_pet.rs");
  const tauri = read("../../../src-tauri/src/lib.rs");
  const catalog = read("../../hooks/providers/useAgentTools.ts");

  assert.match(skill, /astro_tools:\s*\[[^\]]*image_gen[^\]]*desktop_pet/s);
  assert.match(skill, /desktop_pet action=apply/);
  assert.match(seed, /BUNDLED_DESKTOP_PET_CREATOR_MD/);
  assert.match(seed, /"desktop-pet-creator"/);
  assert.match(toolModules, /pub mod desktop_pet;/);
  assert.match(tool, /name: "desktop_pet"/);
  assert.match(tool, /notify_desktop_pet_changed/);
  assert.match(imageTool, /generate_image_with_options/);
  assert.match(imageTool, /reference_images/);
  assert.match(toolGates, /"desktop_pet"/);
  assert.match(desktop, /set_desktop_pet_change_handler/);
  assert.match(desktop, /install_change_bridge/);
  assert.match(tauri, /commands::desktop_pet::install_change_bridge/);
  assert.match(catalog, /id: "desktop_pet"/);
});
