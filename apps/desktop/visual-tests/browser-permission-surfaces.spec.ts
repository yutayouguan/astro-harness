import { expect, test } from "@playwright/test";

for (const theme of ["light", "dark"]) {
  test(`browser permission rows stay transparent (${theme})`, async ({ page }, testInfo) => {
    await page.goto(
      `/iframe.html?id=settings-browsersettingspanel--default&viewMode=story&globals=theme:${theme}`,
    );
    const card = page.locator(".browser-settings-card--permissions");
    const group = card.locator(".browser-permission-toggles");
    const rows = group.locator(".prefs-toggle-row");
    await expect(rows).toHaveCount(2);

    const expectTransparent = async () => {
      for (const surface of [group, rows.nth(0), rows.nth(1)]) {
        await expect(surface).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
        await expect(surface).toHaveCSS("background-image", "none");
        await expect(surface).toHaveCSS("box-shadow", "none");
        await expect(surface).toHaveCSS("backdrop-filter", "none");
      }
    };

    await expectTransparent();
    for (let index = 0; index < 2; index++) {
      await rows.nth(index).hover();
      await expectTransparent();
      const toggle = rows.nth(index).getByRole("switch");
      await toggle.focus();
      await expect(toggle).toBeFocused();
      await expect(toggle).toHaveAttribute("aria-checked", "true");
      await expectTransparent();
    }
    await expect(card).not.toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
    await card.screenshot({ path: testInfo.outputPath(`permissions-${theme}.png`) });
  });
}
