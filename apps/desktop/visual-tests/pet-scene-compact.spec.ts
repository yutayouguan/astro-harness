import { expect, test } from "@playwright/test";

const story =
  "/iframe.html?id=settings-petscenestudio--preview-and-failure&viewMode=story";

for (const theme of ["light", "dark"]) {
  for (const width of [420, 760, 1440]) {
    test(`compact scene cards at ${width}px (${theme})`, async ({ page }, testInfo) => {
      await page.setViewportSize({ width, height: 1100 });
      await page.goto(`${story}&globals=theme:${theme}`);
      await page.getByRole("button", { name: "管理 奶糖", exact: true }).click();
      const cards = page.locator(".pet-scene-card");
      await expect(cards).toHaveCount(2);
      for (const card of await cards.all()) {
        const box = await card.boundingBox();
        expect(box!.width).toBeLessThanOrEqual(281);
        expect(box!.height).toBeLessThan(290);
        const preview = await card.locator(".pet-scene-preview").boundingBox();
        expect(preview!.width / preview!.height).toBeCloseTo(16 / 9, 1);
        for (const button of await card.locator(
          ".pet-scene-card-footer > button, .pet-more > button",
        ).all()) {
          const bounds = await button.boundingBox();
          expect(bounds!.height).toBeGreaterThanOrEqual(28);
          expect(bounds!.height).toBeLessThanOrEqual(30);
          expect(bounds!.x).toBeGreaterThanOrEqual(box!.x);
          expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(box!.x + box!.width);
        }
      }
      const library = page.locator(".pet-detail-view");
      expect(await library.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
      if (width === 1440) {
        const first = await cards.nth(0).boundingBox();
        const second = await cards.nth(1).boundingBox();
        expect(Math.abs(first!.y - second!.y)).toBeLessThan(1);
      }

      // Compact controls retain their existing favorite, disclosure and apply behavior.
      const draft = cards.filter({ has: page.getByRole("heading", { name: "午后书房" }) });
      const favorite = draft.getByRole("button", { name: "收藏场景" });
      await favorite.click();
      await expect(favorite).toHaveAttribute("aria-pressed", "true");
      const more = draft.getByRole("button", { name: "场景更多操作" });
      await more.click();
      await expect(draft.getByRole("button", { name: "编辑场景", exact: true })).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(more).toBeFocused();
      await expect(more).toHaveAttribute("aria-expanded", "false");
      await draft.getByRole("button", { name: "应用整套" }).click();
      await expect(draft.getByText("当前配置", { exact: true })).toBeVisible();
      await library.screenshot({ path: testInfo.outputPath("compact-scenes.png") });
    });
  }
}
