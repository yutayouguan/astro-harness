import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [
  command,
  commandModule,
  tauriLib,
  tauriConfig,
  cargo,
  workflow,
  preferences,
  publicKey,
] = await Promise.all(
  [
    "../../../src-tauri/src/commands/ui/updater.rs",
    "../../../src-tauri/src/commands/mod.rs",
    "../../../src-tauri/src/lib.rs",
    "../../../src-tauri/tauri.conf.json",
    "../../../src-tauri/Cargo.toml",
    "../../../../../.github/workflows/release-tauri.yml",
    "../../components/settings/PreferencesPanel.tsx",
    "../../../src-tauri/updater.pub",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("desktop updater pins a public key and only accepts HTTPS manifests", () => {
  const updaterConfig = JSON.parse(tauriConfig).plugins?.updater;

  assert.match(cargo, /tauri-plugin-updater\s*=\s*"2\.11\.0"/);
  assert.match(tauriLib, /tauri_plugin_updater::Builder::new\(\)/);
  assert.match(tauriLib, /include_str!\("\.\.\/updater\.pub"\)\.trim\(\)/);
  assert.deepEqual(updaterConfig, { pubkey: "" });
  assert.match(command, /endpoint\.scheme\(\) != "https"/);
  assert.match(command, /option_env!\("ASTRO_UPDATE_ENDPOINT"\)/);
  assert.match(command, /yutayouguan\/astro-agent-releases/);
  assert.match(publicKey, /^dW50cnVzdGVkIGNvbW1lbnQ6/);
});

test("update commands check, verify, install, report progress, and restart", () => {
  assert.match(commandModule, /pub\(crate\) use ui::updater;/);
  assert.match(tauriLib, /commands::updater::check_app_update/);
  assert.match(tauriLib, /commands::updater::install_app_update/);
  assert.match(command, /\.download_and_install\(/);
  assert.match(command, /"app-update-progress"/);
  assert.match(command, /app\.restart\(\)/);
  assert.match(preferences, /invoke<AppUpdateInfo>\("check_app_update"\)/);
  assert.match(preferences, /invoke\("install_app_update"\)/);
});

test("release workflow publishes signed updater assets without source", () => {
  assert.equal(JSON.parse(tauriConfig).bundle.createUpdaterArtifacts, true);
  assert.match(workflow, /owner:\s*yutayouguan/);
  assert.match(workflow, /repo:\s*astro-agent-releases/);
  assert.match(workflow, /secrets\.ASTRO_RELEASE_TOKEN/);
  assert.match(workflow, /secrets\.TAURI_SIGNING_PRIVATE_KEY/);
  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY_PASSWORD/);
  assert.match(workflow, /releases\/latest\/download\/latest\.json/);
  assert.match(workflow, /releaseDraft:\s*false/);
  assert.match(workflow, /uploadUpdaterJson:\s*true/);
  assert.doesNotMatch(workflow, /branches:\s*\n\s*-\s*release/);
});
