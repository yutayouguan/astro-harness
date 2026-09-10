import { expect, test } from "@playwright/test";

const story = "/iframe.html?id=settings-petscenestudio--preview-and-failure&viewMode=story";
for (const theme of ["light", "dark"]) {
  test(`pet manager ${theme}: browse without applying and organize scenes`, async ({ page }) => {
    await page.setViewportSize({ width: 1180, height: 860 });
    await page.goto(`${story}&globals=theme:${theme}`);
    await expect(page.getByRole("button", { name: "管理 奶糖", exact: true })).toBeVisible();
    await expect(page.getByRole("heading", { name: "把熟悉的它，带到桌面上" })).toBeVisible();
    await page.screenshot({ path: `test-results/pet-manager-${theme}-library.png`, fullPage: true });
    await page.getByRole("button", { name: "管理 奶糖", exact: true }).click();
    await expect(page.getByRole("heading", { name: "森林小屋", exact: false })).toBeVisible();
    await expect(page.locator(".pet-manager-current")).toContainText("森林小屋");
    await page.getByRole("tab", { name: "动作与偏好" }).click();
    await expect(page.getByRole("button", { name: "保存配置" })).toBeVisible();
    await page.locator(".pet-motion-settings").scrollIntoViewIfNeeded();
    await page.screenshot({ path: `test-results/pet-manager-${theme}-motion.png` });
    await expect(page.locator(".pet-manager-current")).toContainText("森林小屋");
    await page.getByRole("tab", { name: "场景", exact: true }).click();
    await page.getByRole("button", { name: "新增场景" }).click();
    await page.getByRole("textbox", { name: "新场景名称" }).fill("窗边午睡");
    await page.getByRole("button", { name: "保存", exact: true }).click();
    await expect(page.getByRole("heading", { name: "窗边午睡" })).toBeVisible();
    await expect(page.locator(".pet-manager-current")).toContainText("森林小屋");
    await page.screenshot({ path: `test-results/pet-manager-${theme}-scenes.png`, fullPage: true });
    await page.getByRole("tab", { name: "创建宠物" }).click();
    await expect(page.getByRole("button", { name: "生成静态形象" })).toBeVisible();
    await page.locator(".pet-create-card").scrollIntoViewIfNeeded();
    await page.screenshot({ path: `test-results/pet-manager-${theme}-create.png` });
    await page.getByRole("tab", { name: "通用设置" }).click();
    await expect(page.getByRole("checkbox", { name: "全屏时自动隐藏" })).toBeVisible();
    await expect(page.getByRole("slider")).toHaveCount(0);
    await page.screenshot({ path: `test-results/pet-manager-${theme}-general.png` });
  });
}
test("pet manager narrow layout and menu Escape", async ({ page }) => {
  await page.setViewportSize({ width: 540, height: 900 });
  await page.goto(story);
  await page.getByRole("button", { name: "管理 奶糖", exact: true }).click();
  const more = page.getByRole("button", { name: "宠物更多操作", exact: true });
  await more.click();
  await expect(page.getByRole("button", { name: "重命名", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(more).toBeFocused();
  await expect(more).toHaveAttribute("aria-expanded", "false");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: "test-results/pet-manager-narrow.png", fullPage: true });
});

test("wide pet content uses available width with side-by-side creation and preferences", async ({ page }) => {
  await page.setViewportSize({ width: 1560, height: 1100 });
  await page.goto(story);
  await page.getByRole("tab", { name: "创建宠物" }).click();
  const manager = await page.locator(".pet-manager").boundingBox();
  const card = await page.locator(".pet-create-card").boundingBox();
  expect(card!.width).toBeGreaterThan(manager!.width * 0.97);
  const photo = await page.locator(".pet-create-source").boundingBox();
  const fields = await page.locator(".pet-create-fields").boundingBox();
  expect(fields!.x).toBeGreaterThan(photo!.x + photo!.width);
  await page.getByRole("tab", { name: "通用设置" }).click();
  const rows = page.locator(".pet-general-options > label");
  const first = await rows.nth(0).boundingBox(), second = await rows.nth(1).boundingBox();
  expect(second!.x).toBeGreaterThan(first!.x + first!.width);
  expect(Math.abs(second!.y - first!.y)).toBeLessThan(2);
});
