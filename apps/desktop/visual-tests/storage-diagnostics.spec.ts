import { expect, test } from "@playwright/test";

test("storage inspection explains protected data and only previews cleanup", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1100, height: 1100 });
  await page.goto("/iframe.html?id=settings-storagediagnostics--ready&viewMode=story");
  await expect(page.getByRole("heading", { name: "存储与配置" })).toBeVisible();
  await expect(page.getByText("只读检查 · 不迁移、不修改配置、不删除文件")).toBeVisible();
  await expect(page.getByText("资源引用已失效", { exact: true })).toBeVisible();
  await page.getByText(/^清理预览/).click();
  await expect(page.getByText("models/cache/old-model-metadata.json")).toBeVisible();
  await page.getByText("保留策略", { exact: true }).click();
  await expect(page.getByText(/TTL（秒）: 600/)).toBeVisible();
  await expect(page.getByText("/Volumes/cache/astro-mcp", { exact: true })).toBeVisible();
  await expect(page.getByText("数据根以外的目录未扫描", { exact: true })).toBeVisible();
  await expect(page.getByText(/部分缓存目录或文件未验证/)).toBeVisible();
  await expect(page.getByText(/会话、数据库、rollout、工作区/)).toBeVisible();
  await expect(page.getByRole("button", { name: /删除|清理|Delete|Clean/ })).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath("storage-diagnostics.png"), fullPage: true });
});

test("invalid config and limited scans have explicit visible states", async ({ page }) => {
  await page.goto("/iframe.html?id=settings-storagediagnostics--invalid-config&viewMode=story");
  await expect(page.getByText("配置无效", { exact: true })).toBeVisible();
  await expect(page.getByText(/检查不会用默认值覆盖原文件/)).toBeVisible();
  await page.goto("/iframe.html?id=settings-storagediagnostics--partial&viewMode=story");
  await expect(page.getByText(/以下大小仅为已扫描部分/)).toBeVisible();
});

test("narrow storage panel wraps long paths without horizontal overflow", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 900 });
  await page.goto("/iframe.html?id=settings-storagediagnostics--ready&viewMode=story");
  await expect(page.getByRole("heading", { name: "存储与配置" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
});
