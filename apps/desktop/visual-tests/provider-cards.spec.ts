import { expect, test } from "@playwright/test";

for (const theme of ["light", "dark"]) {
  for (const locale of ["zh", "en"]) {
    test(`provider cards stay two-line and compact in ${theme}/${locale}`, async ({ page }) => {
      await page.setViewportSize({ width: 1180, height: 800 });
      await page.addInitScript((language) => localStorage.setItem("astro-locale", language), locale);
      await page.goto(`/iframe.html?id=settings-providerspanel--compact&viewMode=story&globals=theme:${theme}`);
      const card = page.locator('[data-provider-id="azure"] .providers-list-item');
      await expect(card).toBeVisible();
      await expect(card).toHaveAttribute("aria-pressed", "true");
      const badge = card.locator(".providers-badge");
      await expect(badge).toHaveText(locale === "zh" ? "默认" : "Default");
      const bounds = await card.boundingBox();
      expect(bounds!.height).toBeGreaterThanOrEqual(64);
      expect(bounds!.height).toBeLessThanOrEqual(72);
      const name = await card.locator(".providers-list-label").boundingBox();
      const badgeBounds = await badge.boundingBox();
      expect(Math.abs(name!.y - badgeBounds!.y)).toBeLessThan(4);
      await expect(card.locator(".providers-status-dot")).toHaveCSS("width", "6px");
      await expect(card.locator(".providers-status-dot")).toHaveAttribute("aria-label", /.+/);
      await expect(card.locator(".providers-list-icon")).toHaveCSS("opacity", "1");
      const before = await card.locator(".providers-list-text").boundingBox();
      await card.hover();
      await expect(card.locator(".providers-drag-handle")).toHaveCSS("opacity", "0.85");
      await expect(card.locator(".providers-list-icon")).toHaveCSS("opacity", "0");
      expect(await card.locator(".providers-list-text").boundingBox()).toEqual(before);
      await page.mouse.move(1000, 750);
      await page.locator(".providers-pane-list").screenshot({ path: `test-results/provider-cards-${theme}-${locale}.png` });
      await card.screenshot({ path: `test-results/provider-card-${theme}-${locale}.png` });
    });
  }
}

test("long names and models truncate without growing the cards or losing labels", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  await page.goto("/iframe.html?id=settings-providerspanel--long-names&viewMode=story");
  const card = page.locator('[data-provider-id="azure"] .providers-list-item');
  await expect(card).toBeVisible();
  await expect(card.locator(".providers-list-label")).toHaveAttribute("title", "Azure OpenAI Enterprise Production Workspace");
  await expect(card.locator(".providers-list-label")).toHaveCSS("text-overflow", "ellipsis");
  expect((await card.boundingBox())!.height).toBeLessThanOrEqual(72);
  await expect(page.locator('[data-provider-id="openai"] .providers-list-model')).toHaveAttribute("title", /long-deployment/);
  await card.focus();
  await expect(card).toHaveCSS("outline-style", "solid");
  await page.keyboard.press("Tab");
  const second = page.locator('[data-provider-id="openai"] .providers-list-item');
  await expect(second).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(second).toHaveAttribute("aria-pressed", "true");
  await page.emulateMedia({ contrast: "more", reducedMotion: "reduce" });
  await expect(second).toHaveCSS("border-top-color", await second.evaluate((el) => getComputedStyle(el).outlineColor));
  await page.setViewportSize({ width: 600, height: 800 });
  expect((await card.boundingBox())!.height).toBeLessThanOrEqual(72);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(600);
});

test("the compact drag handle still reorders providers and the ghost stays two-line", async ({ page }) => {
  await page.setViewportSize({ width: 1180, height: 800 });
  await page.goto("/iframe.html?id=settings-providerspanel--compact&viewMode=story");
  const azure = page.locator('[data-provider-id="azure"]');
  const openai = page.locator('[data-provider-id="openai"]');
  await expect(openai.locator(".providers-status-dot")).toHaveClass(/ok/);
  await openai.hover();
  const handle = await openai.locator(".providers-drag-handle").boundingBox();
  const target = await azure.boundingBox();
  await page.mouse.move(handle!.x + handle!.width / 2, handle!.y + handle!.height / 2);
  await page.mouse.down();
  const ghost = page.locator(".providers-drag-ghost");
  await expect(ghost).toBeVisible();
  expect((await ghost.boundingBox())!.height).toBeLessThanOrEqual(72);
  await page.mouse.move(target!.x + 40, target!.y + 4, { steps: 5 });
  await page.mouse.up();
  await expect(ghost).toHaveCount(0);
  await expect(page.locator(".providers-list > li").first()).toHaveAttribute("data-provider-id", "openai");
});
