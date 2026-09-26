import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { readI18nCatalogs } from "./i18nCatalogSource.mjs";

const source = async (relativePath) =>
  readFile(new URL(`../../${relativePath}`, import.meta.url), "utf8");

test("screenshot surfaces use locale keys instead of hardcoded Chinese copy", async () => {
  const [
    providers,
    preferences,
    agentInfo,
    projectFiles,
    app,
    messages,
    settingsMessages,
  ] =
    await Promise.all([
      source("components/settings/ProvidersPanel.tsx"),
      source("components/settings/PreferencesPanel.tsx"),
      source("components/chat/ChatAgentInfo.tsx"),
      source("components/chat/ProjectFilesPanel.tsx"),
      source("App.tsx"),
      readI18nCatalogs(),
      source("i18n/catalogs/settings.ts"),
    ]);
  const catalogs = `${messages}\n${settingsMessages}`;

  for (const [component, hardcodedCopy] of [
    [providers, />\s*聊天后备\s*<\/h4>/],
    [providers, /["`]失败时按序切换/],
    [preferences, />\s*侧栏默认显示会话数\s*</],
    [agentInfo, />\s*加载 Agent 信息/],
    [projectFiles, /(?:placeholder|aria-label)="筛选文件/],
    [projectFiles, />\s*当前项目未配置目录\s*</],
  ]) {
    assert.doesNotMatch(component, hardcodedCopy);
  }

  assert.match(providers, /t\("providers\.fallback\.title"\)/);
  assert.match(
    preferences,
    /t\("prefs\.chat\.sidebarVisibleSessionsCount",\s*\{/,
  );
  assert.match(agentInfo, /t\("chat\.rightPanel\.agentLoading"\)/);
  assert.match(projectFiles, /t\("chat\.projectFiles\.noDirectories"\)/);
  assert.match(app, /t\("chat\.projectFiles\.open"\)/);

  for (const key of [
    "providers.fallback.title",
    "prefs.chat.sidebarVisibleSessions",
    "chat.rightPanel.agentLoading",
    "chat.projectFiles.title",
    "chat.projectFiles.noDirectories",
  ]) {
    assert.equal(
      catalogs.match(new RegExp(`"${key.replaceAll(".", "\\.")}":`, "g"))
        ?.length,
      2,
      `${key} must have zh and en translations`,
    );
  }
});
