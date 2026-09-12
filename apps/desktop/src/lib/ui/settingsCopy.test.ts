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

const panelCopyKeys = [
  "environmentDependencies.item.uv",
  "environmentDependencies.item.rtk",
  "environmentDependencies.item.fd",
  "environmentDependencies.item.ripgrep",
  "environmentDependencies.item.bun",
  "environmentDependencies.item.larkCli",
  "environmentDependencies.install.restart",
  "environmentDependencies.unavailable.npm",
  "environmentDependencies.unavailable.installer",
  "environmentDependencies.note",
  "terminal.settings.mode.systemDesc",
  "terminal.settings.font.familyDesc",
  "providers.mediaHint",
  "providers.asrModelHint",
  "providers.embeddingHint",
  "providers.embeddingModelHint",
  "aux.workflowAiPolishDesc",
  "aux.subtitle",
  "aux.routesSub",
  "aux.compactionThresholdHint",
  "aux.backgroundReviewHint",
  "memory.dream.enabledHint",
  "memory.enhanceDiaryTip",
  "memory.pickExpertHintDiaryAll",
  "memory.pending.emptyHintOn",
  "memory.pending.emptyHint",
  "tools.loading.fixed",
  "tools.loading.unavailable",
  "skills.installedEmpty",
  "skills.storeSub",
  "skills.previewBinary",
  "modelMarket.runtime.rerankUnavailable",
  "modelMarket.runtime.embeddingAvailable",
] as const;
const panelRemovedKeys = [
  "environmentDependencies.subtitle",
  "terminal.settings.mode.sub",
  "terminal.settings.behavior.sub",
  "providers.detailSub",
  "providers.voiceHint",
] as const;

test("secondary settings panels use short bilingual descriptions", () => {
  for (const key of panelCopyKeys) {
    assert.ok(zh[key].length > 0 && zh[key].length <= 42, key);
    assert.ok(en[key].length > 0 && en[key].length <= 105, key);
    const params = (text: string) =>
      [...text.matchAll(/\{([^}]+)\}/g)].map((m) => m[1]).sort();
    assert.deepEqual(params(zh[key]), params(en[key]), key);
  }
  assert.match(zh["environmentDependencies.install.restart"], /\{name\}/);
  assert.match(zh["environmentDependencies.install.restart"], /安装命令已完成/);
});

test("redundant panel subtitles are absent without removing controls", async () => {
  const files = [
    "EnvironmentDependenciesPanel.tsx",
    "TerminalSettingsPanel.tsx",
    "ProvidersPanel.tsx",
  ];
  const sources = await Promise.all(
    files.map((file) => read("../../components/settings/" + file)),
  );
  for (const key of panelRemovedKeys) {
    assert.equal(key in zh, false, key);
    assert.equal(key in en, false, key);
    for (const source of sources)
      assert.equal(source.includes(key), false, key);
  }
  assert.match(sources[0], /t\("environmentDependencies.note"\)/);
  assert.match(sources[1], /role="radiogroup"/);
  assert.match(sources[2], /t\("providers.ttsModel"\)/);
});

test("short panel copy retains installation, sandbox, cost and overwrite boundaries", () => {
  assert.match(zh["environmentDependencies.note"], /下方展示.*白名单安装命令/);
  assert.match(
    en["environmentDependencies.note"],
    /allowlisted command shown below/,
  );
  assert.match(
    zh["terminal.settings.mode.projectNote"],
    /Agent 终端始终保持项目沙箱/,
  );
  assert.match(
    zh["aux.backgroundReviewHint"],
    /默认关闭.*每轮.*可能额外收费.*记忆设置/,
  );
  assert.match(
    en["aux.backgroundReviewHint"],
    /Off by default.*each turn.*cost.*Memory settings/,
  );
  assert.match(zh["skills.updateLocalChangesBody"], /更新会覆盖本地文件/);
  assert.match(zh["memory.pending.hint"], /批准后才写入 MEMORY\/USER/);
  assert.match(zh["modelMarket.runtime.rerankUnavailable"], /暂不支持/);
});

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
