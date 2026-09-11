import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { builtStyleHrefs } from "../scripts/check-style-layers.mjs";

const html = readFileSync(new URL("../dist/index.html", import.meta.url), "utf8");
const links = builtStyleHrefs(html).map((href) => `<link rel="stylesheet" href="${href}">`).join("");
const controls = `
  <section class="pet-manager"><div class="pet-library">
    <div class="pet-library-card-footer"><button data-font-probe="pet-manage" class="pet-library-manage">管理场景</button><button data-font-probe="pet-switch" class="pet-library-switch">切换到桌面</button></div>
    <div class="pet-library-toolbar"><label class="pet-library-search"><input data-font-probe="pet-search" type="search" placeholder="搜索宠物名字…"></label><div class="pet-library-filter"><button data-font-probe="pet-filter" class="select-menu-trigger">全部宠物</button></div><button data-font-probe="pet-add" class="desktop-pet-import-package">添加宠物</button></div>
  </div></section>
  <section class="model-picker-group"><button data-font-probe="model-group" class="model-picker-group-label"><span>AZURE OPENAI</span><span class="model-picker-group-count">432</span></button></section>
  <section class="composer-bar"><div class="composer-bar-left"><div class="composer-mode"><button data-font-probe="composer-policy" class="composer-mode-pill composer-policy-pill"><span class="composer-mode-pill-label">Agent</span> · 请求批准</button></div></div></section>
  <section class="browser-address-row"><input data-font-probe="browser-address" placeholder="输入网址，例如 localhost:5173"></section>
  <section class="desktop-ambience"><footer class="ambience-footer"><button data-font-probe="ambience-undo" disabled>撤销上一步</button><button data-font-probe="ambience-manage">管理场景与壁纸</button></footer></section>`;

for (const theme of ["light", "dark"]) {
  test(`production CSS preserves control typography in ${theme}`, async ({ page }) => {
    // Deliberately do not add a layer prelude here: the built CSS must own it.
    // No React/Tauri scripts, real user storage, or application commands run.
    await page.route("**/__font-regression.html", (route) => route.fulfill({
      contentType: "text/html",
      body: `<!doctype html><html data-theme="${theme}"><head><meta charset="utf-8">${links}</head><body>${controls}</body></html>`,
    }));
    await page.goto("/__font-regression.html");
    await expect(page.locator("html")).toHaveCSS("zoom", "1");
    for (const [name, size] of [
      ["pet-manage", "13px"], ["pet-switch", "13px"], ["pet-search", "13px"],
      ["pet-filter", "13px"], ["pet-add", "13px"], ["model-group", "10px"],
      ["composer-policy", "12.48px"], ["browser-address", "11.5px"],
      ["ambience-undo", "11px"], ["ambience-manage", "11px"],
    ]) {
      await test.step(name, async () => {
        await expect(page.locator(`[data-font-probe="${name}"]`)).toHaveCSS("font-size", size);
      });
    }
    await expect(page.locator('[data-font-probe="pet-manage"]')).toHaveCSS("font-weight", "650");
    await expect(page.locator('[data-font-probe="model-group"]')).toHaveCSS("font-weight", "650");
    await expect(page.locator('[data-font-probe="composer-policy"]')).toHaveCSS("font-weight", "600");
  });
}
