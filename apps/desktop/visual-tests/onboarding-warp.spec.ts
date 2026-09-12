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
  await expect(page.locator(".onboarding-stage--provider")).toHaveCSS("transform", "none");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  await page.getByLabel("服务地址", { exact: true }).fill("https://example.com/v1");
  await expect(page.getByLabel("服务地址", { exact: true })).toHaveValue("https://example.com/v1");
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await expect(page.locator(".onboarding-warp")).toHaveAttribute("data-direction", "backward");
  await expect(page.getByRole("heading", { name: "先让这里更像你的工作空间" })).toBeVisible();
  await expect(page.locator(".onboarding-stage--personalize")).toHaveCSS("transform", "none");
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
      await expect(page.locator(".onboarding-app-aperture")).not.toHaveCSS("clip-path", "none");
      const host = page.locator(".onboarding-app-host");
      await host.evaluate(el => el.getAnimations().forEach(animation => { animation.pause(); animation.currentTime = 420; }));
      const projection = await host.evaluate(el => el.getBoundingClientRect().width / (el as HTMLElement).offsetWidth);
      expect(projection).toBeGreaterThan(0.5);
      expect(projection).toBeLessThan(0.95);
      await page.locator(".onboarding-warp").evaluate(el => el.getAnimations({ subtree: true }).forEach(animation => { animation.pause(); animation.currentTime = 450; }));
      await page.screenshot({ path: testInfo.outputPath("app-arrival.png") });
    }
    await expect(page.locator(".onboarding-root")).toHaveCount(0);
    await expect(page.locator(".onboarding-brand-motion")).toHaveCount(0);
    await expect(page.locator(".onboarding-app-host")).toHaveCount(1);
    await expect(page.locator(".onboarding-app-host")).toHaveAttribute("data-arriving", "false");
    await expect(page.locator(".onboarding-app-host")).toHaveCSS("display", "contents");
    await expect(page.locator(".onboarding-app-host")).toHaveCSS("transform", "none");
    await expect(page.locator(".onboarding-app-aperture")).toHaveCSS("clip-path", "none");
    const input = page.getByRole("textbox", { name: "聊天输入框（演示）" });
    await expect(input).toHaveValue(/至少 5 个/);
    await input.fill("修改后的草稿");
    await expect(input).toHaveValue("修改后的草稿");
    await page.getByRole("button", { name: "重新体验" }).click();
    await page.getByRole("button", { name: "进入 Astro，认识一下", exact: true }).click();
    await expect(page.locator(".onboarding-root")).toHaveCount(0);
    await expect(input).toHaveValue("");
  });
}

test("suppressed CSS animation cannot trap App entry", async ({ page }) => {
  await page.goto("/iframe.html?id=app-first-run-onboarding--enter-app&viewMode=story");
  await page.addStyleTag({ content: ".onboarding-warp { animation: none !important; }" });
  await page.getByRole("button", { name: "进入 Astro，认识一下", exact: true }).click();
  await expect(page.locator(".onboarding-root")).toHaveCount(0, { timeout: 4000 });
  await expect(page.getByRole("textbox", { name: "聊天输入框（演示）" })).toBeEditable();
});

test("pages share a flight corridor with an inert departing plane", async ({ page }, testInfo) => {
  await page.goto("/iframe.html?id=app-first-run-onboarding--personalize&viewMode=story");
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await page.locator(".onboarding-scenes").evaluate(async el => {
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    el.getAnimations({ subtree: true }).forEach(animation => { animation.pause(); animation.currentTime = 360; });
  });
  const departing = page.locator('.onboarding-scene-window[data-exiting="true"]');
  const arriving = page.locator('.onboarding-scene-window[data-exiting="false"]');
  await expect(departing).toHaveCount(1);
  await expect(departing).toHaveAttribute("inert", "");
  await expect(departing).toHaveAttribute("aria-hidden", "true");
  const oldScale = await departing.locator(".onboarding-scene-aperture > section").evaluate(el => el.getBoundingClientRect().width / (el as HTMLElement).offsetWidth);
  const newScale = await arriving.locator(".onboarding-scene-aperture > section").evaluate(el => el.getBoundingClientRect().width / (el as HTMLElement).offsetWidth);
  expect(oldScale).toBeGreaterThan(1.08);
  expect(newScale).toBeLessThan(0.95);
  await expect(departing.locator(".onboarding-heading")).toHaveCSS("opacity", "0");
  await expect(arriving.locator(".onboarding-scene-aperture")).not.toHaveCSS("clip-path", "none");
  const boundary = await arriving.evaluate(el => {
    const aperture = el.querySelector(".onboarding-scene-aperture")!;
    const clip = getComputedStyle(aperture).clipPath;
    const radius = Number(clip.match(/circle\(([\d.]+)px/)?.[1]);
    const rim = el.querySelector(".onboarding-portal-rim")!.getBoundingClientRect();
    return { clip, radius, rimRadius: rim.width / 2 };
  });
  expect(Math.abs(boundary.radius - boundary.rimRadius), JSON.stringify(boundary)).toBeLessThan(2);
  await page.screenshot({ path: testInfo.outputPath("page-corridor.png") });
  await page.locator(".onboarding-scenes").evaluate(el => el.getAnimations({ subtree: true }).forEach(animation => animation.play()));
  await expect(departing).toHaveCount(0);
  expect(await arriving.locator(".onboarding-scene-aperture > section").evaluate(el => el.scrollTop)).toBe(0);
  await expect(arriving.locator(".onboarding-scene-aperture")).toHaveCSS("clip-path", "none");
  await expect(arriving.locator(".onboarding-stage")).toHaveCSS("transform", "none");
  await expect(page.getByRole("heading", { name: "为 Astro 接入思考能力" })).toBeInViewport();
  await page.getByLabel("服务地址", { exact: true }).fill("https://example.com/v1");
  await expect(page.getByLabel("服务地址", { exact: true })).toHaveValue("https://example.com/v1");
});

test("system reduced motion removes portals without adding a manual switch", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/iframe.html?id=app-first-run-onboarding--personalize&viewMode=story");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeVisible();
  await expect(page.getByRole("switch", { name: /简洁动效/ })).toHaveCount(0);
  await expect(page.locator(".onboarding-portal-rim")).toHaveCount(0);
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.getByRole("heading", { name: "为 Astro 接入思考能力" })).toBeVisible();
  await expect(page.locator('.onboarding-scene-window[data-exiting="false"] .onboarding-scene-aperture')).toHaveCSS("clip-path", "none");
  await expect(page.locator(".onboarding-portal-rim")).toHaveCount(0);
});

test("interrupted arrival and re-entry settle back to a plain 2D form", async ({ page }) => {
  await page.goto("/iframe.html?id=app-first-run-onboarding--personalize&viewMode=story");
  await page.getByRole("button", { name: "继续", exact: true }).click();
  // Trigger a reversal while the page is still arriving, not after Playwright's stability wait.
  await page.getByRole("button", { name: "返回", exact: true }).evaluate(button => button.click());
  await expect(page.locator(".onboarding-stage--provider")).toHaveCount(0);
  await expect(page.locator(".onboarding-stage--personalize")).toHaveCSS("transform", "none");
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.locator(".onboarding-stage--provider")).toHaveCSS("transform", "none");
  await page.getByLabel("服务地址", { exact: true }).fill("https://example.com/v1");
  await expect(page.locator(".onboarding-stage--provider")).toHaveCSS("transform", "none");
});
