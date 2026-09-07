import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("AI wallpaper and theme chain is wired from bundled skill to Desktop", () => {
  const skill = read(
    "../../../../../crates/agent-skills/bundled/ui-style-designer/SKILL.md",
  );
  const tool = read(
    "../../../../../crates/agent-tools/src/builtin/ui_style.rs",
  );
  const commands = read("../../../src-tauri/src/commands/ui/ui_style.rs");
  const tauri = read("../../../src-tauri/src/lib.rs");
  const hook = read("../../hooks/app/useActiveUiStyle.ts");
  const app = read("../../App.tsx");
  const catalog = read("../../hooks/providers/useAgentTools.ts");

  assert.match(skill, /astro_tools:\s*\[[^\]]*image_gen[^\]]*ui_style/s);
  assert.match(skill, /wallpaperPath/);
  assert.match(tool, /name: "ui_style"/);
  assert.match(tool, /UiStyleAction::Rollback/);
  assert.match(tool, /notify_ui_style_changed/);
  assert.match(commands, /get_active_ui_style/);
  assert.match(tauri, /commands::ui_style::get_active_ui_style/);
  assert.match(hook, /listen\(UI_STYLE_CHANGED_EVENT/);
  assert.match(app, /resolveWallpaperPresentation/);
  assert.match(catalog, /id: "ui_style"/);
});
