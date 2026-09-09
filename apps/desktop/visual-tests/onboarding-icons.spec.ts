import { test, expect } from "@playwright/test";

for (const theme of ["light", "dark"] as const) {
  for (const step of ["personalize", "provider", "workspace", "complete"] as const) {
    test(`onboarding icon layout · ${step} · ${theme}`, async ({ page }, testInfo) => {
      await page.addInitScript(theme => {
        localStorage.setItem("astro-locale", "zh");
        localStorage.setItem("astro-theme-mode", theme);
      }, theme);
      await page.emulateMedia({ reducedMotion: "reduce" });
      await page.setViewportSize({ width: theme === "dark" ? 390 : 1100, height: 1400 });
      await page.goto(`/iframe.html?id=app-first-run-onboarding--${step}&viewMode=story`);
      const card = page.locator(".onboarding-card");
      await expect(card).toBeVisible();
      expect(await card.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
      if (step === "provider") {
        await expect(card.locator(".onboarding-field-label > svg")).toHaveCount(4);
        await expect(page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).locator("svg.lucide-list-restart")).toHaveCount(1);
        await expect(page.getByLabel("服务地址", { exact: true })).toBeEditable();
      }
      if (step === "workspace") {
        await expect(card.locator(".onboarding-workspace-check > svg.lucide-info")).toHaveCount(1);
        await expect(card.locator(".onboarding-permissions strong > svg")).toHaveCount(2);
        await expect(page.getByRole("radio", { name: /执行前询问/ })).toBeChecked();
        await page.getByRole("radio", { name: /自动处理常规操作/ }).check();
        await expect(page.getByRole("radio", { name: /自动处理常规操作/ })).toBeChecked();
        await page.getByRole("radio", { name: /执行前询问/ }).check();
      }
      if (step === "complete") {
        await expect(card.locator(".onboarding-health-grid > div > svg")).toHaveCount(4);
        await expect(card.locator(".onboarding-health-grid")).not.toContainText("○");
        await expect(page.getByRole("button", { name: "填入新对话", exact: true }).locator("svg.lucide-message-square-plus")).toHaveCount(1);
        await expect(card.locator(".onboarding-task-preview summary svg.lucide-eye")).toHaveCount(1);
      }
      for (const icon of await card.locator(".onboarding-field-label > svg, .onboarding-inline-label > svg, .onboarding-workspace-check > svg").all()) {
        await expect(icon).toHaveAttribute("aria-hidden", "true");
      }
      await card.screenshot({ path: testInfo.outputPath("icons.png"), animations: "disabled" });
    });
  }
}
