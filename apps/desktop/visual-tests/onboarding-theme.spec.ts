import { test, expect } from "@playwright/test";

for (const setting of [
  { mode: "dark", system: "light", expected: "dark" },
  { mode: "auto", system: "dark", expected: "dark" },
  { mode: "light", system: "dark", expected: "light" },
] as const) {
  for (const step of ["intro", "personalize", "provider", "workspace", "complete"] as const) {
    test(`onboarding covers a pale native underlay · ${setting.mode}/${setting.system} · ${step}`, async ({ page }, testInfo) => {
      await page.addInitScript(({ mode }) => {
        localStorage.setItem("astro-locale", "zh");
        localStorage.setItem("astro-theme-mode", mode);
      }, setting);
      await page.emulateMedia({ colorScheme: setting.system, reducedMotion: "reduce" });
      await page.setViewportSize({ width: 1100, height: 800 });
      await page.goto(`/iframe.html?id=app-first-run-onboarding--${step}&viewMode=story`);
      // Model the native window's pale backing independently of the DOM theme.
      await page.addStyleTag({ content: "html, body, #storybook-root { background: #dbeafe !important; }" });
      await expect(page.locator("html")).toHaveAttribute("data-theme", setting.expected);
      await expect(page.locator(".onboarding-root h1")).toBeVisible();
      await expect(page.locator(".onboarding-brand-wordmark")).toBeVisible();
      await page.evaluate(() => {
        // This is an image token in the real app, never a color-mix operand.
        document.documentElement.style.setProperty("--shell-bg", "linear-gradient(145deg, #dbeafe, #c7d2fe)");
        document.documentElement.style.setProperty("--unified-tone", "#14b8a6");
        document.documentElement.style.setProperty("--unified-accent-2", "#06b6d4");
      });
      expect(await page.locator(".onboarding-root").evaluate(el => getComputedStyle(el).getPropertyValue("--tone").trim())).toBe("#14b8a6");
      const colors = await page.evaluate(() => {
        const root = document.querySelector(".onboarding-root")!;
        const ink = document.querySelector(".onboarding-brand-wordmark")!;
        const canvas = document.createElement("canvas");
        canvas.width = canvas.height = 1;
        const ctx = canvas.getContext("2d")!;
        const rgba = (color: string) => {
          ctx.clearRect(0, 0, 1, 1); ctx.fillStyle = color; ctx.fillRect(0, 0, 1, 1);
          return [...ctx.getImageData(0, 0, 1, 1).data];
        };
        const luminance = (color: number[]) => color.slice(0, 3).map(v => {
          const c = v / 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
        }).reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
        const background = rgba(getComputedStyle(root).backgroundColor);
        const foreground = rgba(getComputedStyle(ink).color);
        const a = luminance(background), b = luminance(foreground);
        const card = document.querySelector(".onboarding-card");
        return { background, backgroundImage: getComputedStyle(root).backgroundImage,
          contrast: (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05),
          cardAlpha: card ? rgba(getComputedStyle(card).backgroundColor)[3] : 255 };
      });
      expect(colors.background[3]).toBe(255);
      expect(colors.backgroundImage).not.toBe("none");
      expect(colors.cardAlpha).toBeGreaterThan(180);
      expect(colors.contrast).toBeGreaterThan(4.5);
      if (setting.expected === "dark") expect(Math.max(...colors.background.slice(0, 3))).toBeLessThan(70);
      else expect(Math.min(...colors.background.slice(0, 3))).toBeGreaterThan(200);
      await page.screenshot({ path: testInfo.outputPath(`${step}-${setting.expected}.png`) });
    });
  }
}

test("following the system switches the entire onboarding palette", async ({ page }) => {
  await page.addInitScript(() => { localStorage.setItem("astro-theme-mode", "auto"); localStorage.setItem("astro-locale", "zh"); });
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  await page.goto("/iframe.html?id=app-first-run-onboarding--personalize&viewMode=story");
  await expect(page.locator(".onboarding-root")).toHaveCSS("background-color", "rgb(238, 242, 247)");
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page.locator(".onboarding-root")).toHaveCSS("background-color", "rgb(18, 10, 28)");
  await page.getByRole("button", { name: "浅色", exact: true }).click();
  await expect(page.locator(".onboarding-root")).toHaveCSS("background-color", "rgb(238, 242, 247)");
});
