import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (relative) => readFile(new URL(relative, import.meta.url), "utf8");

const app = await read("../../App.tsx");
const projectDialog = await read("../../components/chat/ProjectEditDialog.tsx");
const providers = await read("../../components/settings/ProvidersPanel.tsx");
const evolution = await read(
  "../../components/settings/EvolutionModelsPanel.tsx",
);
const zh = await read("../../i18n/catalogs/zh.ts");
const en = await read("../../i18n/catalogs/en.ts");

/** 确认弹窗必须在破坏性调用之前，且取消时直接返回。 */
function confirmBefore(source, { titleKey, command }) {
  const pattern = new RegExp(
    `const confirmed = await confirm\\(\\{[\\s\\S]*?t\\("${titleKey.replace(/\./g, "\\.")}"\\)[\\s\\S]*?` +
      `if \\(!confirmed\\) return;[\\s\\S]*?${command}`,
  );
  assert.match(source, pattern, `${command} 之前必须先确认`);
}

test("removing a project asks first in both entry points", () => {
  confirmBefore(app, {
    titleKey: "project.removeTitle",
    command: 'invoke\\("delete_project"',
  });
  confirmBefore(projectDialog, {
    titleKey: "project.removeTitle",
    command: 'invoke\\("delete_project"',
  });
});

test("clearing a provider key asks first", () => {
  confirmBefore(providers, {
    titleKey: "providers.clearKeyTitle",
    command: "clear_provider_api_key",
  });
});

test("deleting an eval example asks first", () => {
  confirmBefore(evolution, {
    titleKey: "evo.removeTitle",
    command: "removeEval\\(id\\)",
  });
  // 列表按钮改走带确认的入口
  assert.match(
    evolution,
    /onClick=\{\(\) => void confirmRemoveEval\(ex\.id\)\}/,
  );
});

test("the new confirm copy exists in both locales", () => {
  for (const key of [
    "project.removeTitle",
    "project.removeConfirm",
    "project.removeAction",
    "project.removeKeep",
    "providers.clearKeyTitle",
    "providers.clearKeyConfirm",
    "evo.removeTitle",
    "evo.removeConfirm",
  ]) {
    assert.ok(zh.includes(`"${key}":`), `zh missing ${key}`);
    assert.ok(en.includes(`"${key}":`), `en missing ${key}`);
  }
});
