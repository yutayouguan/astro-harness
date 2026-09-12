import { expect, test } from "@playwright/test";

const story =
  "/iframe.html?id=settings-petscenestudio--preview-and-failure&viewMode=story";

for (const theme of ["light", "dark"]) {
  for (const width of [420, 800, 1440]) {
    test(`compact motion preferences at ${width}px (${theme})`, async ({ page }, testInfo) => {
      await page.setViewportSize({ width, height: 1100 });
      await page.goto(`${story}&globals=theme:${theme}`);
      await page.getByRole("button", { name: "管理 布丁", exact: true }).click();
      await page.getByRole("tab", { name: "动作与偏好", exact: true }).click();
      // Exercise the Soft material shown in the reference as well as the
      // default material covered by pet-motion-layout.spec.ts.
      await page.evaluate(() => { document.documentElement.dataset.material = "soft"; });
      const panel = page.locator(".pet-motion-settings");
      const preview = page.locator(".pet-motion-preview");
      const preferences = page.locator(".pet-motion-preferences");
      expect((await panel.boundingBox())!.width).toBeLessThanOrEqual(960);
      expect((await page.locator(".pet-motion-portrait").boundingBox())!.width).toBeLessThanOrEqual(144);
      expect((await preview.boundingBox())!.height).toBeLessThan(330);
      if (width === 1440) {
        expect((await preview.boundingBox())!.width).toBe(240);
        expect((await preferences.boundingBox())!.height).toBeLessThan(460);
      } else {
        const a = (await preview.boundingBox())!, b = (await preferences.boundingBox())!;
        expect(b.y).toBeGreaterThanOrEqual(a.y + a.height);
      }
      expect(await panel.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
      for (const name of ["摇尾巴", "歪头", "伸懒腰", "趴下打盹", "待机 / 眨眼"]) {
        const button = preview.getByRole("button", { name, exact: true });
        const box = (await button.boundingBox())!;
        expect(box.height).toBeGreaterThanOrEqual(28);
        expect(box.height).toBeLessThanOrEqual(30);
        await button.click();
        await expect(button).toHaveAttribute("aria-pressed", "true");
      }
      const save = page.getByRole("button", { name: "保存配置", exact: true });
      expect((await save.boundingBox())!.height).toBeGreaterThanOrEqual(28);
      expect((await save.boundingBox())!.height).toBeLessThanOrEqual(30);
      await expect(save).toBeDisabled();
      const quiet = page.getByRole("switch", { name: "安静模式", exact: true });
      await quiet.focus();
      await page.keyboard.press("Space");
      await expect(quiet).toHaveAttribute("aria-checked", "true");
      await expect(page.getByRole("combobox", { name: "自动动作间隔" })).toBeDisabled();
      await expect(save).toBeEnabled();
      // The smaller switch's thumb must fit within the track in both states.
      for (const checked of [true, false]) {
        if (!checked) await quiet.click();
        const thumb = quiet.locator(".prefs-switch-thumb");
        await expect(thumb).toHaveCSS("transform", checked ? "matrix(1, 0, 0, 1, 14, 0)" : "none");
        const trackBox = (await quiet.boundingBox())!, thumbBox = (await thumb.boundingBox())!;
        expect(trackBox.height).toBeGreaterThanOrEqual(24);
        expect(thumbBox.x).toBeGreaterThanOrEqual(trackBox.x + 1);
        expect(thumbBox.x + thumbBox.width).toBeLessThanOrEqual(trackBox.x + trackBox.width - 1);
      }
      await page.getByRole("switch", { name: "锁定位置", exact: true }).click();
      await save.click();
      await expect(save).toBeDisabled();
      await expect(page.getByText("当前配置已保存", { exact: true })).toBeVisible();
      await expect(page.locator(".pet-manager-current")).toContainText("奶糖");
      await page.getByRole("tab", { name: "场景", exact: true }).click();
      await page.getByRole("tab", { name: "动作与偏好", exact: true }).click();
      await expect(page.getByRole("switch", { name: "锁定位置", exact: true })).toHaveAttribute("aria-checked", "true");
      await expect.poll(() => page.locator("canvas.pet-motion-portrait").evaluate((node) => {
        const canvas = node as HTMLCanvasElement;
        return canvas.getContext("2d")!.getImageData(0, 0, canvas.width, canvas.height).data.some((value, index) => index % 4 === 3 && value > 0);
      })).toBe(true);
      await panel.screenshot({ path: testInfo.outputPath("compact-motion.png") });
    });
  }
}
