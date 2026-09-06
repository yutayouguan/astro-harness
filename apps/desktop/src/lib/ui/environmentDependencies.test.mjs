import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const component = await readFile(
  new URL(
    "../../components/settings/EnvironmentDependenciesPanel.tsx",
    import.meta.url,
  ),
  "utf8",
);
const backend = await readFile(
  new URL(
    "../../../src-tauri/src/commands/environment_dependencies.rs",
    import.meta.url,
  ),
  "utf8",
);
const tabs = await readFile(
  new URL("settingsTabs.ts", import.meta.url),
  "utf8",
);
const nav = await readFile(new URL("navConfig.ts", import.meta.url), "utf8");

test("environment dependency settings are reachable from the System group", () => {
  assert.match(nav, /\| "environment-dependencies"/);
  assert.match(
    tabs,
    /id: "system"[\s\S]*id: "environment-dependencies"[\s\S]*Icon: IconDependencies/,
  );
});

test("environment dependency UI exposes detection, allowlisted install, and official docs", () => {
  assert.match(
    component,
    /invoke<EnvironmentDependency\[\]>\("list_environment_dependencies"\)/,
  );
  assert.match(component, /"install_environment_dependency"/);
  assert.match(component, /\{ dependencyId: dependency\.id \}/);
  assert.match(component, /openUrl\(meta\.officialUrl\)/);
  assert.match(component, /dependency\.installPath/);
  assert.match(component, /dependency\.installCommand/);
});

test("backend catalog is fixed and installation cannot execute a frontend command", () => {
  for (const id of ["uv", "rtk", "fd", "ripgrep", "bun", "lark-cli"]) {
    assert.match(backend, new RegExp(`id: "${id}"`));
  }
  assert.match(backend, /find\(\|spec\| spec\.id == dependency_id\)/);
  assert.doesNotMatch(
    backend,
    /pub async fn install_environment_dependency\([\s\S]*command: String/,
  );
  assert.match(backend, /INSTALL_TIMEOUT/);
});
