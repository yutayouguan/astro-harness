import { expect, test } from "@playwright/test";

// 项目弹窗的名称行是组合字段：焦点边归外壳。历史缺陷是焦点环画在内部裸 input 上，
// 于是「项目名称」旁边出现一圈 1px 方框（用户截图里的矩形）。
const story =
  "/iframe.html?id=design-projectdialogfocus--name-field-focus&viewMode=story";

test("project name field leaves no inner frame; the row carries the focus edge", async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 900, height: 720 });
  await page.goto(story);

  const row = page.locator(".project-edit-name-row");
  const input = page.locator(".project-edit-name-input");
  await expect(row).toBeVisible();

  await input.click();
  await expect(input).toBeFocused();

  // 焦点标记只在外壳上，输入框自身不参与绘制。
  expect(await input.getAttribute("data-input-focus")).toBeNull();
  expect(await row.getAttribute("data-input-focus")).not.toBeNull();
  expect(
    await input.evaluate((el) => getComputedStyle(el).outlineStyle),
  ).toBe("none");
  expect(await input.evaluate((el) => getComputedStyle(el).boxShadow)).toBe(
    "none",
  );

  // 外壳确实画出焦点边：老 WebView 用描边，新引擎用 border-area 描边。
  const edge = await row.evaluate((el) => {
    const style = getComputedStyle(el);
    return {
      outlineStyle: style.outlineStyle,
      outlineColor: style.outlineColor,
      hasStrokePaint: style.backgroundImage !== "none",
    };
  });
  expect(edge.outlineStyle).toBe("solid");
  expect(
    edge.outlineColor !== "rgba(0, 0, 0, 0)" || edge.hasStrokePaint,
  ).toBe(true);

  // Tab 导航加强到 2px 描边时不能推动布局。
  const pointerBox = await input.boundingBox();
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Tab");
  await expect(input).toBeFocused();
  expect(
    await page.evaluate(() => document.documentElement.dataset.focusModality),
  ).toBe("keyboard");
  const keyboardBox = await input.boundingBox();
  expect(Math.abs((pointerBox?.width ?? 0) - (keyboardBox?.width ?? 0))).toBeLessThan(
    0.5,
  );
  expect(
    Math.abs((pointerBox?.height ?? 0) - (keyboardBox?.height ?? 0)),
  ).toBeLessThan(0.5);

  await page.keyboard.press("ArrowRight");
  await page.screenshot({
    path: testInfo.outputPath("project-dialog-focus.png"),
    fullPage: true,
  });
});

test("icon picker search row carries the edge for its bare input", async ({
  page,
}) => {
  await page.goto(story);
  await page.locator(".project-edit-name-icon-btn").click();

  const shell = page.locator(".project-icon-picker-search");
  const input = shell.locator("input");
  await expect(input).toBeFocused();

  expect(await shell.getAttribute("data-input-focus")).not.toBeNull();
  expect(await input.getAttribute("data-input-focus")).toBeNull();
  expect(
    await input.evaluate((el) => getComputedStyle(el).outlineStyle),
  ).toBe("none");
});
