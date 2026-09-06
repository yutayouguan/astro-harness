import { expect, test } from "@playwright/test";

for (const theme of ["light", "dark"] as const) {
  test(`design baseline · ${theme}`, async ({ page }) => {
    await page.goto(
      `/iframe.html?id=design-baseline--current-language&viewMode=story&globals=theme:${theme}`,
    );

    const baseline = page.getByTestId("design-baseline");
    await expect(baseline).toBeVisible();
    await expect(baseline).toHaveScreenshot(`design-baseline-${theme}.png`);
  });
}
