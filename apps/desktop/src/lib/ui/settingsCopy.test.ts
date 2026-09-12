import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { settingsMessages } from "../../i18n/catalogs/settings.ts";
import { zh, en } from "../../i18n/messages.ts";

const conciseKeys = [
  "prefs.appearance.theme.sub",
  "prefs.appearance.scale.sub",
  "prefs.appearance.glass.sub",
  "prefs.appearance.strategy.unifiedSub",
  "prefs.appearance.strategy.dynamicSub",
  "prefs.appearance.strategy.colorfulSub",
  "prefs.morphicons.sub",
  "prefs.theme.sub",
  "prefs.theme.lightDesc",
  "prefs.theme.darkDesc",
  "prefs.wallpaper.sub",
  "prefs.wallpaper.colorModeDesc",
  "prefs.wallpaper.followSystemDesc",
  "prefs.wallpaper.paletteAutoDesc",
  "prefs.wallpaper.paletteCustomDesc",
  "prefs.wallpaper.aiSub",
  "prefs.wallpaper.generatingHint",
  "prefs.colorStyle.dynamicHint",
  "prefs.colorStyle.editorSub",
  "prefs.appIcon.sub",
  "prefs.appIcon.finderNote",
  "prefs.app.about",
  "prefs.chat.layout.sub",
  "prefs.chat.layout.groupedDesc",
  "prefs.chat.sendModeDesc",
  "prefs.context.sub",
  "prefs.context.viewHint",
  "prefs.context.enabledDesc",
  "prefs.diag.sub",
  "prefs.diag.export.sub",
] as const;
const removedKeys = [
  "prefs.appearance.material.sub",
  "prefs.appearance.motion.sub",
  "prefs.appearance.motion.springSub",
  "prefs.appearance.motion.strokeSub",
  "prefs.chat.sub",
  "prefs.lang.sub",
  "prefs.system.sub",
] as const;
const read = (path: string) => readFile(new URL(path, import.meta.url), "utf8");

test("routine settings descriptions are concise in both languages", () => {
  for (const key of conciseKeys) {
    const chinese = settingsMessages.zh[key];
    const english = settingsMessages.en[key];
    assert.ok(chinese.length > 0 && chinese.length <= 48, key);
    assert.ok(english.length > 0 && english.length <= 100, key);
    assert.doesNotMatch(chinese, /Glassmorphism|卫生参数|全局 Shell|参考教程/);
  }
});

test("settings catalogs keep matching keys and interpolation parameters", () => {
  assert.deepEqual(
    Object.keys(settingsMessages.zh).sort(),
    Object.keys(settingsMessages.en).sort(),
  );
  for (const key of Object.keys(
    settingsMessages.zh,
  ) as (keyof typeof settingsMessages.zh)[]) {
    const params = (text: string) =>
      [...text.matchAll(/\{([^}]+)\}/g)].map((m) => m[1]).sort();
    assert.deepEqual(
      params(settingsMessages.zh[key]),
      params(settingsMessages.en[key]),
      key,
    );
  }
});

test("redundant setting helpers are removed rather than rendered as empty text", async () => {
  const source = await read("../../components/settings/PreferencesPanel.tsx");
  for (const key of removedKeys) {
    assert.equal(key in settingsMessages.zh, false, key);
    assert.equal(key in settingsMessages.en, false, key);
    assert.equal(source.includes(key), false, key);
  }
  assert.doesNotMatch(source, /<p className="prefs-card-sub">\s*<\/p>/);
  // The retained scale description still labels its slider.
  assert.match(source, /id="appearance-scale-description"/);
  assert.match(source, /aria-describedby="appearance-scale-description"/);
});

test("welcome has no instruction footer while card selection remains intact", async () => {
  const source = await read("../../components/chat/SoftWelcome.tsx");
  const css = await read("../../styles/materials/soft-welcome.css");
  assert.doesNotMatch(source, /soft-welcome-hint|chat\.softWelcome\.hint/);
  assert.doesNotMatch(css, /soft-welcome-hint/);
  assert.equal("chat.softWelcome.hint" in zh, false);
  assert.equal("chat.softWelcome.hint" in en, false);
  assert.match(source, /onPickCard\(prompt, promptTemplateHints\(prompt\)\)/);
});

test("essential privacy, approval and data-retention notices stay explicit", async () => {
  assert.match(zh["browser.settings.sensitive"], /每次请求批准/);
  assert.match(zh["skills.apiKeySafetyHint"], /不要在聊天中粘贴 API Key/);
  assert.match(settingsMessages.zh["prefs.diag.export.sub"], /不含 API 密钥/);
  assert.match(
    settingsMessages.zh["prefs.context.enabledDesc"],
    /保留原始历史/,
  );
  assert.match(zh["tools.loading.scope"], /工具开关和权限限制/);
  const pet = await read("../../components/settings/PetCreatePanel.tsx");
  assert.match(pet, /className="pet-create-privacy"/);
  assert.ok(
    pet.includes(
      "仅在点击生成后，照片才会发送到所选图片服务商；本地副本与结果保存在本机。",
    ),
  );
  assert.ok(
    pet.includes(
      "Only generating sends your photo to the selected image provider. Local copies and results stay on this device.",
    ),
  );
});
