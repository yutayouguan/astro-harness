import { expect, test, type Page } from "@playwright/test";

// 网页链接默认进入内置浏览器：这里验证接管与不接管的边界（浏览器坞本身由 App 提供）。
const STORY = "/iframe.html?id=in-app-browser-links--default&viewMode=story";
const UNAVAILABLE_STORY =
  "/iframe.html?id=in-app-browser-links--dock-unavailable&viewMode=story";

function openedLinks(page: Page) {
  return page.evaluate(
    () => (window as unknown as { __openedLinks?: string[] }).__openedLinks ?? [],
  );
}

test("web links are routed to the in-app browser instead of navigating", async ({
  page,
}) => {
  await page.goto(STORY);

  await page.getByTestId("link-plain").click();
  await page.getByTestId("link-inner").click();

  expect(await openedLinks(page)).toEqual([
    "https://example.com/news",
    "https://example.com/inner",
  ]);
  // 默认行为被阻止：故事页面没有被导航走
  expect(page.url()).toContain("viewMode=story");
  await expect(page.getByTestId("link-plain")).toBeVisible();
});

test("modifier clicks are left to the webview default behaviour", async ({
  page,
}) => {
  await page.goto(STORY);

  // Meta/Ctrl 点击不接管：既不记录为内置浏览器打开，具体默认行为由 WebView 决定
  // （macOS Chromium 为「新标签页打开」，headless 下不一定真的开标签，故只断言未接管）。
  const popup = page.context().waitForEvent("page").catch(() => null);
  await page.getByTestId("link-fallback").click({ modifiers: ["Meta"] });

  expect(await openedLinks(page)).toEqual([]);
  const opened = await Promise.race([
    popup,
    page.waitForTimeout(500).then(() => null),
  ]);
  await opened?.close();
});

test("links stay untouched when the in-app browser is unavailable", async ({
  page,
}) => {
  await page.goto(UNAVAILABLE_STORY);

  await page.getByTestId("link-fallback").click();

  expect(await openedLinks(page)).toEqual([]);
  await expect.poll(() => page.url()).toContain("fallback=1");
});
