import { expect, test } from "@playwright/test";

for (const theme of ["light", "dark"]) {
  test(`Soft frost affects surfaces, not wallpaper (${theme})`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.addInitScript((theme) => {
      localStorage.setItem("astro-interface-material", "soft");
      localStorage.setItem("astro-theme-mode", theme);
      localStorage.setItem("astro-soft-frost-intensity", "50");
    }, theme);
    await page.goto(`/iframe.html?id=design-soft-material--playground&viewMode=story&globals=theme:${theme}`);
    await page.getByRole("button", { name: "示例壁纸", exact: true }).click();
    const image = page.locator(".shell-wallpaper-layer > img");
    await expect.poll(() => image.evaluate((el) => (el as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
    const overlay = page.locator(".shell-wallpaper-layer > span");
    const frost = page.locator("#appearance-glass-intensity");
    const card = page.locator(".prefs-card--appearance-material");
    const button = page.locator(".composer-mode-pill");
    const input = page.locator(".composer:not(.has-clarify)");
    const readSurfaces = async () => Promise.all([card, button, input].map((el) =>
      el.evaluate(async (node) => {
        await Promise.all(node.getAnimations().map((animation) => animation.finished.catch(() => {})));
        const css = getComputedStyle(node);
        return {
          background: css.backgroundColor,
          backdrop: css.backdropFilter,
          opacity: css.opacity,
          filter: css.filter,
        };
      }),
    ));
    const exposedWallpaper = { x: 4, y: 80, width: 12, height: 120 };
    const backdropPixels = await page.screenshot({ clip: exposedWallpaper, animations: "disabled" });
    const base = await readSurfaces();
    await expect(card).toHaveCSS("backdrop-filter", "blur(32px) saturate(1.12)");
    await frost.press("End");
    await expect(page.locator("html")).toHaveAttribute("data-soft-frost-intensity", "100");
    await expect(card).toHaveCSS("backdrop-filter", "blur(64px) saturate(1.12)");
    const strong = await readSurfaces();
    for (let index = 0; index < base.length; index++) {
      expect(strong[index].background).not.toBe(base[index].background);
      expect(strong[index].opacity).toBe("1");
      expect(strong[index].filter).toBe("none");
    }
    await expect(button).toHaveCSS("backdrop-filter", "none");
    await expect(image).toHaveCSS("filter", "blur(0px)");
    await expect(overlay).toHaveCSS("backdrop-filter", "none");
    expect(await page.screenshot({ clip: exposedWallpaper, animations: "disabled" })).toEqual(backdropPixels);

    const wallpaperBlur = page.getByRole("slider", { name: /^柔化背景/ });
    await wallpaperBlur.press("End");
    await expect(image).toHaveCSS("filter", "blur(12px)");
    expect(await readSurfaces()).toEqual(strong);
    expect(await page.evaluate(() => localStorage.getItem("astro-soft-frost-intensity"))).toBe("100");
    await frost.press("Home");
    await expect(card).toHaveCSS("backdrop-filter", "none");
    await expect(image).toHaveCSS("filter", "blur(12px)");
    await expect(wallpaperBlur).toHaveValue("12");
    await expect(overlay).toHaveCSS("backdrop-filter", "none");
    await page.screenshot({ path: testInfo.outputPath("independent-frost-and-wallpaper.png") });

    await frost.press("End");
    await page.emulateMedia({ contrast: "more", reducedMotion: "reduce" });
    await expect(card).toHaveCSS("backdrop-filter", "none");
    await expect(image).toHaveCSS("filter", "blur(12px)");
  });
}
