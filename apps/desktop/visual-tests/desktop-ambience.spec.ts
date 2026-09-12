import { expect, test } from "@playwright/test";

async function open(page: import("@playwright/test").Page, variant = "default", theme = "light") {
  await page.goto(`/iframe.html?id=shell-desktopambience--${variant}&viewMode=story&globals=theme:${theme}`);
  const trigger = page.getByRole("button", { name: "桌面氛围", exact: true });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: "桌面氛围" });
  await expect(dialog).toBeVisible();
  return { dialog, trigger };
}

test("single click opens; scene applies both and undo restores; Escape returns focus", async ({ page }) => {
  const { dialog, trigger } = await open(page);
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
  await dialog.getByRole("button", { name: "晴日草地", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("布丁");
  await dialog.getByRole("button", { name: "撤销上一步" }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0); await expect(trigger).toBeFocused();
});

test("wallpaper-only preserves pet; palette keeps wallpaper; save as never overwrites", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("tab", { name: "壁纸", exact: true }).click();
  await dialog.getByRole("button", { name: "晴日草地 宠物场景" }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖 · 晴日草地");
  await dialog.getByRole("tab", { name: "配色", exact: true }).click();
  await dialog.getByRole("button", { name: /灵动配色/ }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖 · 晴日草地");
  await dialog.getByRole("button", { name: "另存为场景" }).click();
  await dialog.getByRole("textbox", { name: "新场景名称" }).fill("草地新配色");
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await dialog.getByRole("tab", { name: "场景", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "草地新配色", exact: true })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "午后窗台", exact: true })).toBeVisible();
});

test("linked wallpaper explicitly switches the pet; tab navigation works", async ({ page }) => {
  const { dialog } = await open(page);
  const scenes = dialog.getByRole("tab", { name: "场景", exact: true });
  await scenes.focus(); await page.keyboard.press("ArrowRight");
  await expect(dialog.getByRole("tab", { name: "壁纸", exact: true })).toHaveAttribute("aria-selected", "true");
  await dialog.getByRole("checkbox", { name: "绑定场景的壁纸，同时切换宠物" }).check();
  await dialog.getByRole("button", { name: "晴日草地 宠物场景" }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("布丁");
});

test("failed changes show feedback and preserve current selection", async ({ page }) => {
  const { dialog } = await open(page, "failure");
  await dialog.getByRole("button", { name: "晴日草地", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("切换失败");
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
});

for (const theme of ["light", "dark"]) {
  test(`layout ${theme} fits narrow window with reduced motion`, async ({ page }) => {
    await page.setViewportSize({ width: 480, height: 700 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    const { dialog } = await open(page, "default", theme);
    await expect(dialog.getByRole("button", { name: "晴日草地", exact: true })).toBeEnabled();
    const box = await dialog.boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0); expect(box!.x + box!.width).toBeLessThanOrEqual(480);
    await page.screenshot({ path: `test-results/desktop-ambience-${theme}.png` });
  });
}

test("scene recoloring is marked modified and can be restored", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("tab", { name: "配色", exact: true }).click();
  await dialog.getByRole("button", { name: "应用自定义" }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖 · 午后窗台");
  await expect(dialog.locator(".ambience-current")).toContainText("已调整");
  await dialog.getByRole("button", { name: "还原场景" }).click();
  await expect(dialog.locator(".ambience-current")).not.toContainText("已调整");
});

test("rapid repeat selection does not replace the useful undo checkpoint", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("button", { name: "晴日草地", exact: true }).dblclick();
  await expect(dialog.locator(".ambience-current")).toContainText("布丁");
  await dialog.getByRole("button", { name: "撤销上一步" }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
});

test("shuffle stays within current pet, supports undo, and explains empty pools", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("button", { name: "换一个", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖 · 森林小屋");
  await dialog.getByRole("button", { name: "撤销上一步" }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖 · 午后窗台");
  await dialog.getByRole("button", { name: "晴日草地", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "换一个", exact: true })).toBeDisabled();
  await expect(dialog.locator(".ambience-shuffle")).toContainText("当前宠物没有其他可用场景");
});

test("favorite wallpaper shuffle ignores pet-link option and favorites survive reload and undo", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("tab", { name: "壁纸", exact: true }).click();
  await dialog.getByRole("button", { name: "收藏 晴日草地", exact: true }).click();
  await dialog.getByRole("checkbox", { name: "绑定场景的壁纸，同时切换宠物" }).check();
  await dialog.getByRole("combobox", { name: "换一个的范围" }).selectOption("favorites");
  await dialog.getByRole("button", { name: "换一个", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖 · 晴日草地");
  await dialog.getByRole("button", { name: "撤销上一步" }).click();
  await expect(dialog.getByRole("button", { name: "取消收藏 晴日草地" })).toHaveAttribute("aria-pressed", "true");
  await page.reload();
  await page.getByRole("button", { name: "桌面氛围", exact: true }).click();
  await expect(dialog.getByRole("combobox", { name: "换一个的范围" })).toHaveValue("favorites");
  await dialog.getByRole("tab", { name: "壁纸", exact: true }).click();
  await dialog.getByRole("checkbox", { name: /只看收藏/ }).check();
  await expect(dialog.locator(".ambience-wallpaper-card")).toHaveCount(1);
  await dialog.getByRole("button", { name: "取消收藏 晴日草地" }).click();
  await expect(dialog.locator(".ambience-empty")).toContainText("暂无收藏");
  await expect(dialog.getByRole("button", { name: "换一个", exact: true })).toBeDisabled();
});

test("lock persists but allows explicit picks and dynamic colors", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("checkbox", { name: "锁定随机换景" }).check();
  await expect(dialog.getByRole("button", { name: "换一个", exact: true })).toBeDisabled();
  await dialog.getByRole("button", { name: "森林小屋", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("森林小屋");
  await dialog.getByRole("combobox", { name: "换一个的范围" }).selectOption("palette");
  await dialog.getByRole("button", { name: "换一个", exact: true }).click();
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖 · 森林小屋");
  await expect(dialog.locator(".ambience-current")).toContainText("灵动配色");
  await page.reload();
  await page.getByRole("button", { name: "桌面氛围", exact: true }).click();
  await expect(dialog.getByRole("checkbox", { name: "锁定随机换景" })).toBeChecked();
});
