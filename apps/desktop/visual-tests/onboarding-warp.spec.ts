import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("astro-locale", "zh"));
});

for (const theme of ["light", "dark"] as const) {
  test(`intro hyperspace fills the viewport · ${theme}`, async ({ page }, testInfo) => {
    await page.addInitScript(theme => localStorage.setItem("astro-theme-mode", theme), theme);
    await page.setViewportSize({ width: theme === "dark" ? 1440 : 390, height: 900 });
    await page.goto("/iframe.html?id=app-first-run-onboarding--intro&viewMode=story");
    await expect(page.getByRole("button", { name: "重播动画" })).toBeVisible();
    await page.getByRole("button", { name: "重播动画" }).click();
    const warp = page.locator(".onboarding-warp");
    await expect(warp).toHaveAttribute("data-mode", "intro");
    await expect(warp).toHaveCSS("pointer-events", "none");
    await expect(warp.locator(".onboarding-warp-rings i")).toHaveCount(5);
    await expect(warp.locator(".onboarding-warp-rays i")).toHaveCount(40);
    expect(await warp.boundingBox()).toEqual({ x: 0, y: 0, width: theme === "dark" ? 1440 : 390, height: 900 });
    await warp.evaluate(el => el.getAnimations({ subtree: true }).forEach(animation => { animation.pause(); animation.currentTime = 850; }));
    await page.screenshot({ path: testInfo.outputPath("hyperspace-intro.png") });
    await expect(warp).toHaveCount(0, { timeout: 4000 }); // bounded even if animation is paused
  });
}

test("step travel reverses, stays interactive, and never bypasses model verification", async ({ page }) => {
  await page.goto("/iframe.html?id=app-first-run-onboarding--personalize&viewMode=story");
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.locator(".onboarding-warp")).toHaveAttribute("data-direction", "forward");
  await expect(page.getByRole("heading", { name: "为 Astro 接入思考能力" })).toBeVisible();
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  await page.getByLabel("服务地址", { exact: true }).fill("https://example.com/v1");
  await expect(page.getByLabel("服务地址", { exact: true })).toHaveValue("https://example.com/v1");
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await expect(page.locator(".onboarding-warp")).toHaveAttribute("data-direction", "backward");
  await expect(page.getByRole("heading", { name: "先让这里更像你的工作空间" })).toBeVisible();
  await expect(page.locator(".onboarding-warp")).toHaveCount(0);
  await expect(page.locator(".onboarding-brand-motion")).toHaveCount(1);
});

for (const reduced of [false, true]) {
  test(`App arrival preserves editable starter draft · reduced=${reduced}`, async ({ page }, testInfo) => {
    await page.emulateMedia({ reducedMotion: reduced ? "reduce" : "no-preference" });
    await page.goto("/iframe.html?id=app-first-run-onboarding--enter-app&viewMode=story");
    await page.getByRole("button", { name: "看看我的桌面 · 填入输入框", exact: true }).click();
    if (!reduced) {
      await expect(page.locator(".onboarding-warp")).toHaveAttribute("data-mode", "app");
      await expect(page.locator(".onboarding-app-host")).toHaveAttribute("data-arriving", "true");
      await page.locator(".onboarding-warp").evaluate(el => el.getAnimations({ subtree: true }).forEach(animation => { animation.pause(); animation.currentTime = 450; }));
      await page.screenshot({ path: testInfo.outputPath("app-arrival.png") });
    }
    await expect(page.locator(".onboarding-root")).toHaveCount(0);
    await expect(page.locator(".onboarding-brand-motion")).toHaveCount(0);
    await expect(page.locator(".onboarding-app-host")).toHaveCount(1);
    await expect(page.locator(".onboarding-app-host")).toHaveAttribute("data-arriving", "false");
    const input = page.getByRole("textbox", { name: "聊天输入框（演示）" });
    await expect(input).toHaveValue(/至少 5 个/);
    await input.fill("修改后的草稿");
    await expect(input).toHaveValue("修改后的草稿");
    await page.getByRole("button", { name: "重新体验" }).click();
    await page.getByRole("button", { name: "空白开始", exact: true }).click();
    await expect(page.locator(".onboarding-root")).toHaveCount(0);
    await expect(input).toHaveValue("");
  });
}

test("missing brand target and suppressed CSS animation cannot trap App entry", async ({ page }) => {
  await page.goto("/iframe.html?id=app-first-run-onboarding--enter-app&viewMode=story");
  await page.addStyleTag({ content: ".onboarding-warp { animation: none !important; } [data-onboarding-brand-target] { display: none !important; }" });
  await page.getByRole("button", { name: "空白开始", exact: true }).click();
  await expect(page.locator(".onboarding-root")).toHaveCount(0, { timeout: 4000 });
  await expect(page.getByRole("textbox", { name: "聊天输入框（演示）" })).toBeEditable();
});
