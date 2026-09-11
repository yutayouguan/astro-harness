import { expect, test } from "@playwright/test";

const story = "/iframe.html?id=settings-petscenestudio--empty-photo&viewMode=story";
for (const theme of ["light", "dark"]) {
  test(`create and general settings ${theme}: clear entry points and local help`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 1060 });
    await page.goto(`${story}&globals=theme:${theme}`);
    await page.getByRole("tab", { name: "创建宠物", exact: true }).click();
    await expect(page.getByRole("button", { name: "生成静态形象", exact: true })).toBeDisabled();
    await expect(page.getByRole("button", { name: "导入动画桌宠", exact: true })).toBeEnabled();
    const photo = await page.getByRole("button", { name: "选择照片", exact: true }).boundingBox();
    expect(photo!.height).toBeLessThanOrEqual(220);
    await expect(page.getByRole("textbox", { name: "场景描述", exact: true })).toBeHidden();
    const wallpaper = page.getByRole("switch", { name: "同时生成配套壁纸", exact: true });
    await wallpaper.click();
    await expect(wallpaper).toHaveAttribute("aria-checked", "true");
    await page.getByRole("textbox", { name: "场景描述", exact: true }).fill("森林小屋");
    await wallpaper.click();
    await expect(page.getByRole("textbox", { name: "场景描述", exact: true })).toBeHidden();
    await wallpaper.click();
    await expect(page.getByRole("textbox", { name: "场景描述", exact: true })).toHaveValue("森林小屋");
    await wallpaper.click();
    await page.locator(".pet-create-layout").scrollIntoViewIfNeeded();
    await page.screenshot({ path: `test-results/pet-create-refined-${theme}.png` });

    await page.getByRole("tab", { name: "通用设置", exact: true }).click();
    await expect(page.getByRole("heading", { name: "显示与联动", exact: true })).toBeVisible();
    await expect(page.getByRole("heading", { name: "免打扰", exact: true })).toBeVisible();
    for (const label of ["始终置顶", "随配套壁纸切换宠物", "全屏时自动隐藏", "演示时暂时隐藏"]) {
      const toggle = page.getByRole("switch", { name: label, exact: true });
      const before = await toggle.getAttribute("aria-checked");
      await toggle.focus();
      await page.keyboard.press("Space");
      await expect(toggle).toHaveAttribute("aria-checked", before === "true" ? "false" : "true");
      await expect(toggle).toHaveAttribute("aria-describedby", /.+/);
    }
    await page.getByRole("button", { name: "恢复显示", exact: true }).click();
    await expect(page.getByRole("switch", { name: "演示时暂时隐藏", exact: true })).toHaveAttribute("aria-checked", "false");
    await page.screenshot({ path: `test-results/pet-general-refined-${theme}.png` });
  });
}

test("creation and general settings reflow in a narrow content area", async ({ page }) => {
  await page.setViewportSize({ width: 540, height: 960 });
  await page.goto(story);
  await page.getByRole("tab", { name: "创建宠物", exact: true }).click();
  await page.getByRole("switch", { name: "同时生成配套壁纸", exact: true }).click();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  const source = await page.locator(".pet-create-source").boundingBox();
  const fields = await page.locator(".pet-create-fields").boundingBox();
  expect(fields!.y).toBeGreaterThan(source!.y + source!.height);
  await page.getByRole("tab", { name: "通用设置", exact: true }).click();
  const groups = page.locator(".pet-general-group");
  const first = await groups.nth(0).boundingBox(), second = await groups.nth(1).boundingBox();
  expect(second!.y).toBeGreaterThan(first!.y + first!.height);
  await page.locator(".pet-general-card").scrollIntoViewIfNeeded();
  await page.screenshot({ path: "test-results/pet-general-refined-narrow.png" });
});
