import { expect, test, type Page } from "@playwright/test";

test("pet size changes natively before mouse release and one undo restores the gesture", async ({ page }) => {
  const { dialog } = await open(page, "slow-scale");
  await dialog.getByText("显示调整", { exact: true }).click();
  await dialog.getByRole("button", { name: "适应", exact: true }).click();
  await dialog.getByRole("tab", { name: "宠物", exact: true }).click();
  const restore = dialog.getByRole("button", { name: "还原场景", exact: true });
  const undo = dialog.getByRole("button", { name: "撤销", exact: true });
  await expect(restore).toBeVisible();
  await expect(undo).toBeEnabled();
  const slider = dialog.getByRole("slider", { name: "桌宠大小", exact: true });
  await slider.scrollIntoViewIfNeeded();
  const box = (await slider.boundingBox())!;
  const y = box.y + box.height / 2;
  await page.mouse.move(box.x + box.width - 9, y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.6, y, { steps: 6 });
  const intermediate = await slider.inputValue();
  expect(Number(intermediate)).toBeLessThan(0.3);
  await expect(dialog).toHaveAttribute("data-live-pet-scale", "true");
  await expect(restore).toBeDisabled();
  await expect(restore).toHaveCSS("opacity", "1");
  await expect(undo).toHaveCSS("opacity", "1");
  await expect(dialog.locator('.ambience-scene-tile[data-has-image="true"]').first()).toHaveCSS("opacity", "1");
  await expect(page.getByTestId("native-pet-scale")).toHaveText(intermediate);
  await expect(slider).toBeEnabled();
  await page.mouse.move(box.x + 9, y, { steps: 8 });
  await expect(slider).toHaveValue("0.15");
  await page.mouse.up();
  await expect(page.getByTestId("native-pet-scale")).toHaveText("0.15");
  await dialog.getByRole("button", { name: "撤销", exact: true }).click();
  await expect(slider).toHaveValue("0.3");
  await expect(page.getByTestId("native-pet-scale")).toHaveText("0.3");
});

test("wallpaper slider drags across intermediate values and stays interactive during save", async ({ page }) => {
  const { dialog } = await open(page, "slow-save");
  await dialog.getByText("显示调整", { exact: true }).click();
  const slider = dialog.getByRole("slider", { name: "内容保护", exact: true });
  await slider.scrollIntoViewIfNeeded();
  const box = (await slider.boundingBox())!;
  const x = box.x + 9 + (box.width - 18) * 18 / 55;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.55, y, { steps: 8 });
  const mid = Number(await slider.inputValue());
  expect(mid).toBeGreaterThan(18);
  const layer = page.locator(".shell-wallpaper-layer");
  await expect.poll(() => layer.evaluate(element => element.style.getPropertyValue("--wallpaper-live-shade"))).toBe(String(mid / 100));
  await expect.poll(() => layer.evaluate(element => element.style.getPropertyValue("--wallpaper-shade"))).toBe("0.18");
  await page.mouse.move(box.x + box.width * 0.8, y, { steps: 8 });
  const last = Number(await slider.inputValue());
  expect(last).toBeGreaterThan(mid);
  await page.mouse.up();
  await expect(slider).toBeEnabled();
  await expect(slider).toBeFocused();
  await expect(dialog.locator(".ambience-wallpaper-display")).toHaveAttribute("aria-busy", "false");
  await expect(slider).toHaveValue(String(last));
  await expect.poll(() => layer.evaluate(element => element.style.getPropertyValue("--wallpaper-live-shade"))).toBe("");
  await dialog.getByRole("button", { name: "撤销", exact: true }).click();
  await expect(slider).toHaveValue("18");
});

test("wallpaper display settings preserve the scene and can be undone", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByText("显示调整", { exact: true }).click();
  await dialog.getByRole("button", { name: "适应", exact: true }).click();
  await expect(page.getByTestId("wallpaper-display-preview")).toHaveCSS("object-fit", "contain");
  const shade = dialog.getByRole("slider", { name: "内容保护", exact: true });
  await shade.press("End");
  await expect(shade).toHaveValue("55");
  await dialog.getByRole("button", { name: "撤销", exact: true }).click();
  await expect(shade).toHaveValue("18");
  const blur = dialog.getByRole("slider", { name: "柔化背景", exact: true });
  await blur.press("End");
  await expect(page.getByTestId("wallpaper-display-preview")).toHaveCSS("filter", "blur(12px)");
  await expect(dialog.locator(".ambience-current")).toContainText("奶糖");
  await dialog.getByRole("tab", { name: "宠物", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "还原场景", exact: true })).toBeVisible();
});

test("long names keep scene cards aligned and available in tooltips", async ({ page }) => {
  const { dialog } = await open(page, "long-labels");
  await dialog.getByRole("tab", { name: "宠物", exact: true }).click();
  const names = await dialog.locator(".ambience-scene-tile .ambience-tile-name").all();
  const heights = await Promise.all(names.map(async name => (await name.boundingBox())!.height));
  expect(Math.max(...heights) - Math.min(...heights)).toBeLessThan(1);
  expect(Math.max(...heights)).toBeLessThan(34);
  for (const card of await dialog.locator(".ambience-scene-tile").all()) {
    const name = await card.locator(".ambience-tile-name").textContent();
    await expect(card).toHaveAttribute("title", name!);
  }
});

test("no pet disables resizing and offers a clear settings path", async ({ page }) => {
  const { dialog, trigger } = await open(page, "no-pet");
  await dialog.getByRole("tab", { name: "宠物", exact: true }).click();
  await expect(dialog.getByRole("slider", { name: "桌宠大小", exact: true })).toBeDisabled();
  await expect(dialog.locator(".ambience-empty")).toContainText("还没有桌宠");
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
});

test("expanded random options fit the action area and save form receives focus", async ({ page }) => {
  await page.setViewportSize({ width: 340, height: 480 });
  const { dialog } = await open(page);
  await dialog.getByRole("tab", { name: "宠物", exact: true }).click();
  const more = dialog.getByLabel("随机选项", { exact: true });
  const before = await more.boundingBox();
  await more.click();
  const after = await more.boundingBox();
  expect(after!.x).toBeCloseTo(before!.x, 0);
  const shuffle = await dialog.getByRole("button", { name: "换个场景", exact: true }).boundingBox();
  expect(after!.y).toBeCloseTo(shuffle!.y, 0);
  const action = await dialog.locator(".ambience-context-action").boundingBox();
  const lock = await dialog.getByRole("checkbox", { name: "锁定随机换景" }).boundingBox();
  expect(lock!.y + lock!.height).toBeLessThanOrEqual(action!.y + action!.height);
  await more.click();
  await dialog.getByText("另存当前组合为场景", { exact: true }).click();
  await expect(dialog.getByRole("textbox", { name: "新场景名称" })).toBeFocused();
});

test("tab positions stay stable, pet tiles align and actions remain outside the scroll area", async ({ page }) => {
  await page.setViewportSize({ width: 960, height: 900 });
  const { dialog } = await open(page);
  const tops: number[] = [];
  for (const name of ["材质", "背景", "宠物"]) {
    await dialog.getByRole("tab", { name, exact: true }).click();
    const tabBox = await dialog.getByRole("tablist").boundingBox();
    tops.push(tabBox!.y);
  }
  expect(Math.max(...tops) - Math.min(...tops)).toBeLessThan(2);
  const tabs = await dialog.getByRole("tab").all();
  const widths = await Promise.all(tabs.map(async tab => (await tab.boundingBox())!.width));
  expect(Math.max(...widths) - Math.min(...widths)).toBeLessThan(2);
  const avatars = await dialog.locator(".ambience-pets > button").all();
  const heights = await Promise.all(avatars.map(async button => (await button.boundingBox())!.height));
  expect(Math.max(...heights) - Math.min(...heights)).toBeLessThan(2);
  await expect(dialog.locator(".ambience-content .ambience-context-action")).toHaveCount(0);
  await expect(dialog.getByRole("button", { name: "换个场景", exact: true })).toBeVisible();
  await page.setViewportSize({ width: 340, height: 480 });
  const box = await dialog.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(340);
  expect(box!.height).toBeLessThanOrEqual(480 * 0.75 + 1);
});

test("pet size targets the current pet, updates live and supports undo", async ({ page }) => {
  const { dialog } = await open(page);
  await dialog.getByRole("tab", { name: "宠物", exact: true }).click();
  const slider = dialog.getByRole("slider", { name: "桌宠大小", exact: true });
  await expect(slider).toHaveValue("0.3");
  await slider.press("Home");
  await expect(slider).toHaveAttribute("aria-valuetext", "50%");
  await dialog.getByRole("button", { name: "布丁", exact: true }).click();
  await expect(dialog.locator(".ambience-pet-scale")).toContainText("奶糖");
  await dialog.getByRole("button", { name: "撤销", exact: true }).click();
  await expect(slider).toHaveValue("0.3");
});

test("pet size failure restores the confirmed value", async ({ page }) => {
  const { dialog } = await open(page, "failure");
  await dialog.getByRole("tab", { name: "宠物", exact: true }).click();
  const slider = dialog.getByRole("slider", { name: "桌宠大小", exact: true });
  await slider.press("Home");
  await expect(dialog.getByRole("alert")).toContainText("大小调整失败");
  await expect(slider).toHaveValue("0.3");
});

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

test("picking a wallpaper turns wallpaper colors back on by default", async ({ page }) => {
  const { dialog } = await open(page);
  const toggle = dialog.getByRole("checkbox", { name: "从壁纸自动取色" });

  // 第一次选壁纸：默认从壁纸取色。
  await dialog.getByRole("button", { name: "森林小屋", exact: true }).click();
  await expect(toggle).toBeEnabled();
  await expect(toggle).toBeChecked();

  // 用户手选配色 → 关掉。
  await toggle.uncheck();
  await expect(toggle).not.toBeChecked();

  // 换一张壁纸 → 回到默认的从壁纸取色。
  await dialog.getByRole("button", { name: "晴日草地", exact: true }).click();
  await expect(toggle).toBeChecked();
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
