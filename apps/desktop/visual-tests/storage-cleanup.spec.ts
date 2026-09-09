import { expect, test, type Page } from "@playwright/test";

const calls = (page: Page) => page.evaluate(() => (window as unknown as { storageCleanupCalls: string[] }).storageCleanupCalls);
async function open(page: Page) {
  await page.goto("/iframe.html?id=settings-storagecleanup--confirmation&viewMode=story");
  await page.getByRole("button", { name: "生成清理确认单" }).click();
  await expect(page.getByRole("dialog", { name: "确认移入恢复区" })).toBeVisible();
}
test("preparing a plan does not move files; only acknowledged explicit confirmation does", async ({ page }, testInfo) => {
  await open(page);
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("button", { name: "确认移入恢复区" })).toBeDisabled();
  await expect(dialog.getByText(/磁盘空间不会立即释放/)).toBeVisible();
  await page.keyboard.press("Enter");
  expect(await calls(page)).not.toContain("execute_storage_cleanup");
  await dialog.getByRole("checkbox").check();
  await dialog.getByRole("checkbox").press("Enter");
  expect(await calls(page)).not.toContain("execute_storage_cleanup");
  expect(await dialog.evaluate(node => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("cleanup-confirmation.png"), fullPage: true });
  await dialog.getByRole("button", { name: "确认移入恢复区" }).click();
  await expect(page.getByText("已移入恢复区", { exact: true })).toBeVisible();
  expect((await calls(page)).filter(c => c === "execute_storage_cleanup")).toHaveLength(1);
  await expect(page.getByRole("button", { name: "查看恢复区" })).toBeVisible();
});
test("Enter on Cancel discards the plan instead of executing it", async ({ page }) => {
  await open(page); const dialog = page.getByRole("dialog");
  await dialog.getByRole("checkbox").check();
  await dialog.getByRole("button", { name: "取消", exact: true }).press("Enter");
  await expect(dialog).toHaveCount(0);
  expect(await calls(page)).toContain("discard_storage_cleanup");
  expect(await calls(page)).not.toContain("execute_storage_cleanup");
});
test("confirmation focus is contained and Escape never moves files", async ({ page }) => {
  await open(page); const dialog = page.getByRole("dialog");
  await dialog.getByRole("checkbox").check();
  await dialog.getByRole("button", { name: "确认移入恢复区" }).focus();
  await page.keyboard.press("Tab");
  await expect(dialog.getByRole("checkbox")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  expect(await calls(page)).not.toContain("execute_storage_cleanup");
});

test("withdrawing acknowledgement during the closing animation prevents the queued move", async ({ page }) => {
  await open(page); const dialog = page.getByRole("dialog");
  await dialog.getByRole("checkbox").check();
  await dialog.getByRole("button", { name: "确认移入恢复区" }).click();
  await dialog.getByRole("checkbox").uncheck({ force: true });
  await page.waitForTimeout(250);
  expect(await calls(page)).not.toContain("execute_storage_cleanup");
});

test("long cache names remain reviewable in a narrow confirmation dialog", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 800 });
  await open(page);
  const dialog = page.getByRole("dialog");
  expect(await dialog.evaluate(node => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
  expect(await dialog.locator(".storage-cleanup-list").evaluate(node => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
});
