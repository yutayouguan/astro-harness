import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    // Test-only native transport marker, before hooks evaluate their platform flag.
    Object.assign(window, { __TAURI_INTERNALS__: {} });
  });
});

async function openBrowser(page: import("@playwright/test").Page, story: string) {
  await page.goto(`/iframe.html?id=tools-loading--${story}&viewMode=story`);
  await page.locator(".tools-detail-list").getByText(/网页预览与操作|Browser/, { exact: true }).click();
}

test("group policy saves, survives reload, resets, and rolls back on errors", async ({ page }) => {
  await openBrowser(page, "supported");
  const selector = page.getByRole("button", { name: /工具组加载策略|Tool group loading policy/ });
  await expect(selector).toContainText(/自动|Automatic/);
  await expect(page.getByRole("status")).toContainText(/tool_search/);
  await selector.click();
  await page.getByRole("option", { name: /始终加载|Always load/ }).click();
  await expect(selector).toContainText(/始终加载|Always load/);
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem("qa.toolLoading")!))).toEqual({ browser: "always" });
  await openBrowser(page, "supported");
  await expect(selector).toContainText(/始终加载|Always load/);
  await page.evaluate(() => sessionStorage.setItem("qa.failSave", "1"));
  await selector.click();
  await page.getByRole("option", { name: /按需加载|On demand/ }).click();
  await expect(page.getByRole("alert")).toContainText("QA save failed");
  await expect(selector).toContainText(/始终加载|Always load/);
  await page.evaluate(() => sessionStorage.removeItem("qa.failSave"));
  await selector.click();
  await page.getByRole("option", { name: /自动|Automatic/ }).click();
  await expect(selector).toContainText(/自动|Automatic/);
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem("qa.toolLoading")!))).toEqual({});
});

test("unsupported model visibly reports upfront loading", async ({ page }) => {
  await openBrowser(page, "unsupported");
  await expect(page.getByRole("status")).toContainText(/提前加载|loaded upfront/);
  await page.screenshot({ path: "test-results/tool-loading-unsupported.png", fullPage: true });
});
