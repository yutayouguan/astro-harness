import { expect, test, type Page } from "@playwright/test";

async function open(page: Page, variant = "default") {
  await page.goto("/iframe.html?id=shell-desktopambience--" + variant + "&viewMode=story");
  const trigger = page.getByRole("button", { name: "桌面氛围", exact: true });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "桌面氛围" });
  await expect(dialog).toBeVisible();
  return { dialog, trigger };
}

test("three tabs; browsing pets is non-mutating; scene undo and Escape work", async ({ page }) => {
  const { dialog, trigger } = await open(page);
  await expect(dialog.getByRole("tab")).toHaveCount(3);
  await dialog.getByRole("tab", { name: "宠物", exact: true }).click();
  await dialog.getByRole("button", { name: "布丁", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
  await dialog.getByRole("button", { name: "晴日草地", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("布丁");
  await dialog.getByRole("button", { name: "撤销", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

test("wallpaper-only keeps pet; linked wallpaper switches pet explicitly", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("button", { name: "晴日草地", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
  await expect(dialog.locator(".ambience-current")).toHaveAttribute("title", "晴日草地");
  await dialog.getByRole("button", { name: "撤销", exact: true }).click();
  await dialog.getByRole("checkbox", { name: "关联壁纸同时切换宠物" }).check();
  await dialog.getByRole("button", { name: "晴日草地", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("布丁");
});

test("material strength and mode have local undo without changing wallpaper or pet", async ({ page }) => {
  const { dialog } = await open(page);
  const previous = await dialog.locator(".ambience-current").getAttribute("title");
  await dialog.getByRole("tab", { name: "材质", exact: true }).click();
  await dialog.getByRole("button", { name: "柔塑 Soft", exact: true }).click();
  const slider = dialog.getByRole("slider");
  await slider.press("Home");
  await slider.press("End");
  await expect(slider).toHaveValue("100");
  await dialog.getByRole("button", { name: "撤销", exact: true }).click();
  await expect(slider).toHaveValue("0");
  await dialog.getByRole("button", { name: "深色", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(dialog.locator(".ambience-current")).toHaveAttribute("title", previous!);
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
});

test("ambient strategies live under color backgrounds and can be undone", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("button", { name: "氛围配色", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("氛围配色");
  await dialog.getByRole("button", { name: "统一", exact: true }).click();
  await dialog.getByRole("button", { name: "紫罗兰", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "紫罗兰", exact: true })).toHaveAttribute("aria-pressed", "true");
  await dialog.getByRole("button", { name: "灵动", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "换组颜色", exact: true })).toBeVisible();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
  await dialog.getByRole("button", { name: "撤销", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "统一", exact: true })).toHaveAttribute("aria-pressed", "true");
});

test("favorites shuffle ignores pet linking and survives undo", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("button", { name: "收藏 晴日草地", exact: true }).click();
  await dialog.getByRole("checkbox", { name: "关联壁纸同时切换宠物" }).check();
  await dialog.getByRole("button", { name: "随机收藏", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
  await expect(dialog.locator(".ambience-current")).toHaveAttribute("title", "晴日草地");
  await dialog.getByRole("button", { name: "撤销", exact: true }).click();
  await dialog.getByRole("button", { name: "收藏", exact: true }).click();
  await expect(dialog.locator(".ambience-wallpaper-card")).toHaveCount(1);
  await expect(dialog.getByRole("button", { name: "取消收藏 晴日草地", exact: true })).toHaveAttribute("aria-pressed", "true");
});

test("lock blocks random scenes but not explicit scene choices", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("tab", { name: "宠物", exact: true }).click();
  await dialog.getByLabel("随机选项", { exact: true }).click();
  await dialog.getByRole("checkbox", { name: "锁定随机换景" }).check();
  await expect(dialog.getByRole("button", { name: "换个场景", exact: true })).toBeDisabled();
  await dialog.getByRole("button", { name: "森林小屋", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toHaveAttribute("title", "森林小屋");
});

test("failure preserves background type and selected pet", async ({ page }) => {
  const { dialog } = await open(page, "failure");
  await dialog.getByRole("button", { name: "氛围配色", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("切换失败");
  await expect(dialog.getByRole("button", { name: "图片壁纸", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
});

test("save as retains existing scenes and supports management navigation", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("tab", { name: "宠物", exact: true }).click();
  await dialog.getByText("另存当前组合为场景", { exact: true }).click();
  await dialog.getByRole("textbox", { name: "新场景名称" }).fill("新的组合");
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "新的组合", exact: true })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "午后窗台", exact: true })).toBeVisible();
  await dialog.getByRole("button", { name: "更多设置", exact: true }).click();
  await expect(page.getByTestId("manage-target")).toHaveText("scenes");
});

test("navigation preserves appearance undo and both themes fit a narrow window", async ({ page }) => {
  await page.setViewportSize({ width: 480, height: 700 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  const { dialog } = await open(page);
  await dialog.getByRole("tab", { name: "材质", exact: true }).click();
  await dialog.getByRole("button", { name: "柔塑 Soft", exact: true }).click();
  await dialog.getByRole("button", { name: "浅色", exact: true }).click();
  await dialog.getByRole("button", { name: "深色", exact: true }).click();
  await dialog.getByRole("button", { name: "更多设置", exact: true }).click();
  await page.getByRole("button", { name: "返回对话" }).click();
  await page.getByRole("button", { name: "桌面氛围", exact: true }).click();
  await dialog.getByRole("button", { name: "撤销", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  const box = await dialog.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(480);
  expect(box!.height).toBeLessThanOrEqual(700 * 0.75 + 1);
});
