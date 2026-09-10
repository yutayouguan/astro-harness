import { test, expect } from "@playwright/test";

for (const viewport of [{ width: 1366, height: 768 }, { width: 1024, height: 720 }, { width: 900, height: 700 }]) {
  for (const step of ["personalize", "provider", "workspace", "complete"] as const) {
    test(`compact onboarding fits ${viewport.width}x${viewport.height} · ${step}`, async ({ page }, testInfo) => {
      await page.addInitScript(() => { localStorage.setItem("astro-locale", "zh"); localStorage.setItem("astro-theme-mode", "light"); });
      await page.emulateMedia({ reducedMotion: "reduce" });
      await page.setViewportSize(viewport);
      await page.goto(`/iframe.html?id=app-first-run-onboarding--${step}&viewMode=story`);
      const stage = page.locator(`.onboarding-stage--${step}`);
      await expect(stage).toBeVisible();
      const metrics = await stage.evaluate(el => ({ content: el.scrollHeight, viewport: el.clientHeight, horizontal: el.scrollWidth - el.clientWidth }));
      await page.screenshot({ path: testInfo.outputPath(`${step}.png`) });
      console.log(JSON.stringify({ step, ...viewport, ...metrics }));
      expect(metrics.content - metrics.viewport).toBeLessThanOrEqual(2);
      expect(metrics.horizontal).toBeLessThanOrEqual(2);
      await expect(page.getByRole("button", { name: step === "complete" ? "空白开始" : step === "workspace" ? "完成设置" : "继续", exact: true })).toBeInViewport();
    });
  }
}

for (const step of ["personalize", "provider", "workspace", "complete"] as const) {
  test(`compact English remains readable · ${step}`, async ({ page }, testInfo) => {
    await page.addInitScript(() => { localStorage.setItem("astro-locale", "en"); localStorage.setItem("astro-theme-mode", "dark"); });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.setViewportSize({ width: 1024, height: 720 });
    await page.goto(`/iframe.html?id=app-first-run-onboarding--${step}&viewMode=story`);
    const stage = page.locator(`.onboarding-stage--${step}`);
    await expect(stage).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath(`${step}-en.png`) });
    expect(await stage.evaluate(el => el.scrollHeight - el.clientHeight)).toBeLessThanOrEqual(2);
    expect(await stage.evaluate(el => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(2);
  });
}

test("narrow layout reflows and all preferences remain reachable", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("astro-locale", "zh"));
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 390, height: 700 });
  await page.goto("/iframe.html?id=app-first-run-onboarding--personalize&viewMode=story");
  const stage = page.locator(".onboarding-stage--personalize");
  await expect(stage).toBeVisible();
  expect(await stage.evaluate(el => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(2);
  await page.getByRole("switch", { name: "桌面宠物", exact: true }).click();
  await expect(page.getByRole("switch", { name: "桌面宠物", exact: true })).toBeChecked();
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.getByRole("heading", { name: "为 Astro 接入思考能力" })).toBeVisible();
});
