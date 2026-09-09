import { test, expect } from "@playwright/test";
import { STARTER_TASKS } from "../src/lib/ui/onboardingTasks";

for (const locale of ["zh", "en"] as const) {
  test(`complete examples fill editable preview without sending · ${locale}`, async ({ page }, testInfo) => {
    await page.addInitScript(locale => {
      localStorage.setItem("astro-locale", locale);
      localStorage.setItem("astro-theme-mode", locale === "zh" ? "dark" : "light");
    }, locale);
    await page.setViewportSize({ width: locale === "zh" ? 390 : 1280, height: 900 });
    await page.goto("/iframe.html?id=app-first-run-onboarding--complete&viewMode=story");
    const input = page.getByRole("textbox", { name: locale === "zh" ? "聊天输入框（演示）" : "Chat input (demo)" });
    await expect(page.locator(".onboarding-task-card")).toHaveCount(6);
    for (const task of STARTER_TASKS) {
      await page.getByRole("button", { name: `${task[locale].title} · ${locale === "zh" ? "填入输入框" : "Fill chat input"}`, exact: true }).click();
      await expect(input).toHaveValue(task[locale].prompt);
      await expect(input).toBeFocused();
      await expect(input).toBeEditable();
    }
    await input.fill("Edited draft, not sent");
    await expect(input).toHaveValue("Edited draft, not sent");
    await expect(page.locator(".onboarding-demo-composer").getByRole("status")).toContainText(locale === "zh" ? "不会保存或发送" : "never saves or sends");
    expect(await page.locator(".onboarding-root").evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath("draft-preview.png") });
    await page.reload();
    await expect(page.locator(".onboarding-task-card")).toHaveCount(6);
    await expect(input).toHaveCount(0);
  });
}

test("intro particles cover the viewport, replay, and respect reduced motion", async ({ page }, testInfo) => {
  await page.addInitScript(() => localStorage.setItem("astro-locale", "zh"));
  await page.goto("/iframe.html?id=app-first-run-onboarding--intro&viewMode=story");
  const field = page.locator(".onboarding-particle-field");
  await expect(field.locator("i")).toHaveCount(54);
  for (const viewport of [{ width: 1440, height: 900 }, { width: 390, height: 844 }]) {
    await page.setViewportSize(viewport);
    await expect(field).toHaveCSS("pointer-events", "none");
    expect(await field.boundingBox()).toEqual({ x: 0, y: 0, ...viewport });
    const anchor = page.locator("[data-onboarding-brand-anchor=intro]");
    await expect.poll(async () => {
      const bounds = (await anchor.boundingBox())!;
      const origin = await field.evaluate(el => parseFloat((el as HTMLElement).style.getPropertyValue("--particle-origin-x")));
      return Math.abs(origin - bounds.x - bounds.width / 2);
    }).toBeLessThan(1);
    expect(await page.locator(".onboarding-root").evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole("button", { name: "重播动画" }).click();
  // Freeze the first visible frame to verify particles extend to all viewport edges.
  await field.locator("i").evaluateAll(elements => elements.forEach(el => el.getAnimations().forEach(animation => { animation.pause(); animation.currentTime = 80; })));
  const boxes = await field.locator("i").evaluateAll(elements => elements.map(el => el.getBoundingClientRect().toJSON()));
  expect(Math.min(...boxes.map(box => box.x))).toBeLessThan(144);
  expect(Math.max(...boxes.map(box => box.x))).toBeGreaterThan(1296);
  expect(Math.min(...boxes.map(box => box.y))).toBeLessThan(90);
  expect(Math.max(...boxes.map(box => box.y))).toBeGreaterThan(810);
  await page.screenshot({ path: testInfo.outputPath("intro-fullscreen.png") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect(field.locator("i").first()).toHaveCSS("animation-name", "none");
  await page.getByRole("button", { name: "跳过动画" }).click();
  await expect(field).toHaveCount(0);
  await expect(page.locator(".onboarding-stage--personalize")).toBeVisible();
});
