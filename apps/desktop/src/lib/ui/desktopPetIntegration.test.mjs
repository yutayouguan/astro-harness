import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const app = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");
const main = await readFile(new URL("../../main.tsx", import.meta.url), "utf8");
const panel = await readFile(
  new URL("../../components/settings/DesktopPetPanel.tsx", import.meta.url),
  "utf8",
);
const surface = await readFile(
  new URL(
    "../../components/desktop-pet/DesktopPetSurface.tsx",
    import.meta.url,
  ),
  "utf8",
);
const controller = await readFile(
  new URL("../../hooks/app/useDesktopPetState.ts", import.meta.url),
  "utf8",
);
const threadEvents = await readFile(
  new URL("../../../src-tauri/src/infra/thread_events.rs", import.meta.url),
  "utf8",
);
const commands = await readFile(
  new URL("../../../src-tauri/src/lib.rs", import.meta.url),
  "utf8",
);
const backend = await readFile(
  new URL("../../../src-tauri/src/commands/ui/desktop_pet.rs", import.meta.url),
  "utf8",
);
const provider = await readFile(
  new URL(
    "../../../../../crates/agent-providers/src/openai/image_http.rs",
    import.meta.url,
  ),
  "utf8",
);
const resetCss = await readFile(
  new URL("../../styles/foundation/reset.css", import.meta.url),
  "utf8",
);
const globalsCss = await readFile(
  new URL("../../styles/foundation/globals.css", import.meta.url),
  "utf8",
);

test("opaque root underlays exclude desktop pets in normal, zoom and macOS modes", () => {
  const sharedRoot = resetCss.match(/html,\s*body,\s*#root\s*\{([^}]+)\}/)?.[1];
  assert.ok(sharedRoot);
  assert.doesNotMatch(sharedRoot, /background\s*:/);
  assert.match(
    resetCss,
    /html:not\(\[data-window-surface="desktop-pet"\]\) #root/,
  );
  assert.match(
    globalsCss,
    /html\.zooming:not\(\[data-window-surface="desktop-pet"\]\) #root/,
  );
  for (const rule of globalsCss.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (rule[1].includes("body") && /background\s*:/.test(rule[2])) {
      assert.ok(
        rule[1].includes(':not([data-window-surface="desktop-pet"])'),
        rule[1],
      );
    }
  }
  assert.match(main, /colorScheme\s*=\s*isDesktopPetWindow\s*\?\s*"normal"/);
  assert.match(backend, /\.transparent\(true\)/);
});

test("static assets do not advertise working animation controls", () => {
  const pause = surface.split('id: "pet-pause"')[1].split('id: "pet-hide"')[0];
  assert.match(pause, /enabled: state\.spriteVersionNumber === 2/);
  assert.match(pause, /静态形象/);
  assert.match(panel, /生成静态形象/);
  assert.match(panel, /不包含动画帧/);
});

test("desktop pet is reachable from settings and a dedicated transparent surface", () => {
  assert.match(app, /<DesktopPetPanel active=\{nav === "settings"\}/);
  assert.match(main, /get\("surface"\) === "desktop-pet"/);
  assert.match(main, /<DesktopPetSurface \/>/);
  assert.match(surface, /getCurrentWindow\(\)\s*\.startDragging\(\)/);
  assert.match(surface, /\.onMoved\(/);
  assert.match(surface, /useDesktopPetState\(\)/);
  assert.match(controller, /desktop-pet-changed/);
});

test("desktop pet commands cover import, generation, persistence and window state", () => {
  for (const command of [
    "get_desktop_pet_state",
    "import_desktop_pet_photo",
    "import_desktop_pet_package",
    "set_desktop_pet_enabled",
    "set_desktop_pet_scale",
    "set_desktop_pet_always_on_top",
  ]) {
    assert.match(commands, new RegExp(`commands::desktop_pet::${command}`));
    assert.match(panel + controller, new RegExp(command));
  }
  for (const command of ["create_pet_scene", "generate_pet_scene_wallpaper"]) {
    assert.match(commands, new RegExp(`commands::pet_scene::${command}`));
    assert.match(panel, new RegExp(command));
  }
  assert.match(backend, /types::desktop_pet_root\(base\)/);
  assert.match(backend, /DESKTOP_PET_V2_USED_COLUMNS/);
  assert.match(backend, /WebviewUrl::App\("index\.html\?surface=desktop-pet"/);
  assert.match(backend, /always_on_top\(state\.always_on_top\)/);
});

test("history replay does not emit desktop pet reactions", () => {
  const genericEmitter = threadEvents
    .split("pub(crate) fn emit_chat_events")[1]
    .split("fn emit_live_desktop_pet_events")[0];
  assert.doesNotMatch(
    genericEmitter,
    /app\.emit\(\s*DESKTOP_PET_ACTIVITY_CHANGED_EVENT/,
  );
  assert.match(
    threadEvents,
    /emit_live_desktop_pet_events\(app, &thread_id, &events\)/,
  );
  assert.match(threadEvents, /ts_ms: next_session_status_ts_ms\(\)/);
});

test("native pet creation runs outside synchronous command callbacks", () => {
  assert.match(backend, /pub async fn import_desktop_pet_package/);
  assert.match(backend, /pub async fn set_desktop_pet_enabled/);
  assert.match(backend, /spawn_blocking\(move \|\| refresh_from_disk/);
});

test("personalization sends the uploaded photo through the shared image edit pipeline", () => {
  assert.match(backend, /generate_image_data_with_reference/);
  assert.match(provider, /if edit_mode \{ "edits" \} else \{ "generations" \}/);
  assert.match(provider, /form\.part\("image\[\]", part\)/);
  assert.match(provider, /MAX_INPUT_IMAGE_BYTES/);
  assert.match(panel, /import_desktop_pet_package/);
  assert.match(surface, /<DesktopPetCanvas/);
});
