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
  await dialog.getByRole("checkbox").check();
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
