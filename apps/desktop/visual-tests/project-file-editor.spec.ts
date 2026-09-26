import { expect, test } from "@playwright/test";

// 打开文档后编辑器会拿到焦点，历史上这里出现过两种「选中后的高亮」：
// 1) 输入焦点环给整块正文套 1px 方框；2) CodeMirror 当前行整行铺色。
// 两种都去掉，但真实文本选区必须仍在。
const story =
  "/iframe.html?id=design-project-file-editor--markdown-source&viewMode=story";

test("opening a document leaves no focus frame, but keeps the caret and selection", async ({
  page,
}) => {
  await page.goto(story);
  const content = page.locator(".cm-content");
  await expect(content).toBeVisible();

  // 点在有文字的行上，光标才会落在可继续向右扩选的位置。
  await page.locator(".cm-line").nth(4).click();
  await expect(content).toBeFocused();

  expect(await content.getAttribute("data-input-focus")).toBeNull();
  expect(
    await content.evaluate((el) => getComputedStyle(el).outlineStyle),
  ).toBe("none");

  await expect(page.locator(".cm-activeLine, .cm-activeLineGutter")).toHaveCount(
    0,
  );
  const backgrounds = await page
    .locator(".cm-line")
    .evaluateAll((lines) =>
      lines.map((line) => getComputedStyle(line).backgroundColor),
    );
  expect(new Set(backgrounds)).toEqual(new Set(["rgba(0, 0, 0, 0)"]));

  for (let index = 0; index < 6; index += 1) {
    await page.keyboard.press("Shift+ArrowRight");
  }
  // CodeMirror 自己绘制选区，一段选区可能拆成多个矩形。
  const selection = page.locator(".cm-selectionBackground").first();
  await expect(selection).toBeVisible();
  expect(
    await selection.evaluate((el) => getComputedStyle(el).backgroundColor),
  ).toContain("rgba(139, 92, 246");
});
