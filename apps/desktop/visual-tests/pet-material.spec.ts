import { expect, test } from "@playwright/test";

for (const theme of ["light", "dark"]) {
  test(`pet materials follow wallpaper hues without losing contrast (${theme})`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1280, height: 1050 });
    await page.goto(`/iframe.html?id=settings-petscenestudio--preview-and-failure&viewMode=story&globals=theme:${theme}`);
    const card = page.locator(".pet-library-card").first();
    await expect(card).toBeVisible();
    const colors: string[] = [];
    for (const tone of ["#b77c1c", "#408561", "#768cca"]) {
      await page.evaluate(({ tone, theme }) => {
        const root = document.documentElement;
        root.dataset.theme = theme;
        root.dataset.wallpaper = "true";
        root.dataset.wallpaperPalette = "true";
        root.dataset.glassIntensity = "64";
        root.style.setProperty("--wallpaper-tone", tone);
        root.style.setProperty("--wallpaper-accent-2", tone);
      }, { tone, theme });
      const metrics = await card.evaluate((element) => {
        const css = getComputedStyle(element);
        const canvas = document.createElement("canvas");
        canvas.width = canvas.height = 1;
        const ctx = canvas.getContext("2d")!;
        const rgba = (color: string) => {
          ctx.clearRect(0, 0, 1, 1);
          ctx.fillStyle = color;
          ctx.fillRect(0, 0, 1, 1);
          return [...ctx.getImageData(0, 0, 1, 1).data];
        };
        const bg = rgba(css.backgroundColor);
        const luminance = (rgb: number[]) => rgb.slice(0, 3).map((c) => {
          const n = c / 255;
          return n <= 0.04045 ? n / 12.92 : ((n + 0.055) / 1.055) ** 2.4;
        }).reduce((sum, n, i) => sum + n * [0.2126, 0.7152, 0.0722][i], 0);
        const text = rgba(getComputedStyle(element.querySelector(".pet-library-card-copy > span")!).color);
        const composite = text.slice(0, 3).map((c, i) => c * text[3] / 255 + bg[i] * (1 - text[3] / 255));
        const a = luminance(bg), b = luminance(composite);
        return {
          color: css.backgroundColor, alpha: bg[3], blur: css.backdropFilter,
          contrast: (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05),
        };
      });
      expect(metrics.alpha).toBe(255);
      expect(metrics.blur).toBe("none");
      expect(metrics.contrast).toBeGreaterThanOrEqual(4.5);
      colors.push(metrics.color);
    }
    expect(new Set(colors).size).toBe(3);
    await page.evaluate(() => {
      document.documentElement.style.setProperty("--wallpaper-tone", "#b77c1c");
      const main = document.querySelector<HTMLElement>(".settings-content-inline")!;
      main.style.background = 'url("/src/stories/assets/pet-room.jpeg") center / cover fixed';
    });
    await page.locator(".pet-library").screenshot({ path: testInfo.outputPath("library.png") });
    await page.getByRole("button", { name: "管理 奶糖", exact: true }).click();
    await page.getByRole("tab", { name: "动作与偏好", exact: true }).click();
    const preview = page.locator(".pet-motion-preview");
    await expect(preview).toBeVisible();
    await expect(preview).toHaveCSS("backdrop-filter", "none");
    expect(await preview.evaluate((el) => getComputedStyle(el).backgroundImage)).toContain("radial-gradient");
    const select = page.locator(".pet-detail-interval select");
    const before = await select.evaluate((el) => getComputedStyle(el).backgroundColor);
    await page.evaluate(() => document.documentElement.style.setProperty("--wallpaper-tone", "#408561"));
    expect(await select.evaluate((el) => getComputedStyle(el).backgroundColor)).not.toBe(before);
    await page.locator(".pet-library").screenshot({ path: testInfo.outputPath("motion.png") });
    await page.emulateMedia({ contrast: "more", reducedMotion: "reduce" });
    await expect(preview).toHaveCSS("background-image", "none");
  });
}
