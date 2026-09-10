import { test, expect } from "@playwright/test";

for (const theme of ["light", "dark"] as const) {
  for (const step of ["personalize", "provider", "workspace", "complete"] as const) {
    test(`onboarding shadow fits its scroll gutter · ${theme} · ${step}`, async ({ page }, testInfo) => {
      await page.addInitScript(theme => {
        localStorage.setItem("astro-theme-mode", theme);
        localStorage.setItem("astro-locale", "zh");
      }, theme);
      await page.emulateMedia({ reducedMotion: "reduce" });
      await page.goto(`/iframe.html?id=app-first-run-onboarding--${step}&viewMode=story`);
      const stage = page.locator(`.onboarding-stage--${step}`);
      const card = stage.locator(".onboarding-card");
      await expect(card).toBeVisible();
      for (const width of [1452, 900, 390]) {
        await page.setViewportSize({ width, height: 768 });
        const shadow = await card.evaluate(el => getComputedStyle(el).boxShadow);
        const [x, y, blur, spread] = Array.from(shadow.matchAll(/(-?[\d.]+)px/g), match => Number(match[1]));
        expect([x, y, blur, spread]).toEqual([0, 4, 12, -6]);
        // Approximate the visible Gaussian tail (3 sigma) and reserve that clearance.
        const reach = Math.max(0, 1.5 * blur + spread);
        const bounds = (await stage.boundingBox())!;
        await stage.evaluate(el => { el.scrollTop = 0; });
        let face = (await card.boundingBox())!;
        expect(face.x - bounds.x).toBeGreaterThanOrEqual(reach - x);
        expect(bounds.x + bounds.width - face.x - face.width).toBeGreaterThanOrEqual(reach + x);
        expect(face.y - bounds.y).toBeGreaterThanOrEqual(Math.max(0, reach - y));
        if (width === 1452) await page.screenshot({ path: testInfo.outputPath(`${step}-${theme}.png`) });
        await stage.evaluate(el => { el.scrollTop = el.scrollHeight; });
        face = (await card.boundingBox())!;
        expect(bounds.y + bounds.height - face.y - face.height).toBeGreaterThanOrEqual(reach + y);
        expect(await stage.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
      }
    });
  }
}
