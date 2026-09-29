import { test, expect } from "@playwright/test";

for (const theme of ["light", "dark"] as const) {
  test(`intro lighting matches the logo face at every size · ${theme}`, async ({ page }, testInfo) => {
    await page.addInitScript(theme => {
      localStorage.setItem("astro-theme-mode", theme);
      localStorage.setItem("astro-locale", "zh");
      // Exercise the CSS fallback deterministically, independent of GPU availability.
      Object.defineProperty(navigator, "gpu", { configurable: true, value: undefined });
    }, theme);
    await page.goto("/iframe.html?id=app-first-run-onboarding--intro&viewMode=story");
    const logo = page.locator(".onboarding-brand-motion .onboarding-logo-mark");
    await expect(logo).toBeVisible();
    await expect(page.locator(".onboarding-brand-motion .onboarding-logo-base")).toHaveCount(0);
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 });
      // Pause the whole reveal and sweep on a representative middle frame.
      await logo.evaluate(el => el.getAnimations({ subtree: true }).forEach(animation => {
        animation.pause();
        const end = Number(animation.effect!.getComputedTiming().endTime);
        animation.currentTime = Number.isFinite(end) ? end : 1800;
        if ((animation as CSSAnimation).animationName === "welcome-logo-material-sweep") animation.currentTime = 1800;
      }));
      const front = logo.locator(".chat-welcome-logo--front");
      for (const selector of [".chat-welcome-logo-lighting", ".chat-welcome-logo-lighting-fallback"]) {
        await expect.poll(async () => {
          const a = (await front.boundingBox())!;
          const b = (await logo.locator(selector).boundingBox())!;
          return Math.max(...(["x", "y", "width", "height"] as const).map(key => Math.abs(a[key] - b[key])));
        }).toBeLessThan(0.5);
        await expect(logo.locator(selector)).toHaveCSS("mask-size", "contain");
      }
      await logo.screenshot({ path: testInfo.outputPath(`logo-${width}.png`) });
    }
    await page.emulateMedia({ reducedMotion: "reduce" });
    await expect(logo).toHaveCSS("animation-name", "none");
    await expect(logo.locator(".chat-welcome-logo-lighting-fallback")).toHaveCSS("animation-name", "none");
    await page.getByRole("button", { name: "跳过动画", exact: true }).click();
    await expect(page.locator(".onboarding-brand-motion .onboarding-logo-base")).toHaveCount(1);
    await expect(page.locator(".onboarding-brand-motion .chat-welcome-logo-stack")).toHaveCount(0);
  });
}

for (const reducedMotion of ["reduce", "no-preference"] as const) {
  test(`logo and wordmark stay continuous from intro to setup · ${reducedMotion}`, async ({ page }, testInfo) => {
    await page.addInitScript(() => { localStorage.setItem("astro-locale", "zh"); localStorage.setItem("astro-theme-mode", "dark"); });
    await page.emulateMedia({ reducedMotion });
    await page.goto("/iframe.html?id=app-first-run-onboarding--intro&viewMode=story");
    const wordmark = page.locator(".onboarding-brand-wordmark");
    await expect(wordmark).toHaveText("Astro Harness");
    await expect(wordmark).toHaveAttribute("data-phase", "intro");
    const wordmarkNode = (await wordmark.elementHandle())!;
    const logoNode = (await page.locator(".onboarding-brand-motion").elementHandle())!;
    await page.getByRole("button", { name: "跳过动画", exact: true }).click();
    await expect(wordmark).toHaveAttribute("data-phase", "header");
    for (const width of [1100, 390]) {
      await page.setViewportSize({ width, height: 800 });
      await expect.poll(async () => {
        const actual = (await wordmark.boundingBox())!;
        const target = (await page.locator("[data-onboarding-wordmark-anchor=header]").boundingBox())!;
        return Math.max(...(["x", "y", "width", "height"] as const).map(key => Math.abs(actual[key] - target[key])));
      }).toBeLessThan(1);
      await expect(wordmark).toBeInViewport();
    }
    await page.getByRole("button", { name: "继续", exact: true }).click();
    await expect(page.locator(".onboarding-stage--provider")).toHaveCSS("transform", "none");
    expect(await wordmarkNode.evaluate(el => el.isConnected)).toBe(true);
    expect(await logoNode.evaluate(el => el.isConnected)).toBe(true);
    await expect(page.locator(".onboarding-brand-wordmark")).toHaveCount(1);
    await page.screenshot({ path: testInfo.outputPath("brand-handoff.png") });
  });
}
