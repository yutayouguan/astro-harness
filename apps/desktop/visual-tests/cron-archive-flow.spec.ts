import { expect, test, type Page } from "@playwright/test";

// 定时任务归档 / 删除的交互链路（IPC 由故事 mock，断言的是前端契约与界面状态）。
const STORY =
  "/iframe.html?id=pages-cronpanel--cards-with-detail-drawer&viewMode=story";
const SIDEBAR_STORY =
  "/iframe.html?id=sidebar-session-states--cron-owner-states-story&viewMode=story";

type CronCall = { command: string; payload: unknown };

function readCronCalls(page: Page) {
  return page.evaluate(
    () =>
      (window as unknown as { __cronCalls?: CronCall[] }).__cronCalls ?? [],
  );
}

async function callsFor(page: Page, command: string) {
  return (await readCronCalls(page))
    .filter((call) => call.command === command)
    .map((call) => call.payload);
}

test("archived scheduled tasks stay out of the default list and can be restored", async ({
  page,
}) => {
  await page.goto(STORY);

  // 归档任务默认不出现，但工具栏给出已归档入口与数量
  await expect(page.locator(".cron-card")).toHaveCount(2);
  const filter = page.getByRole("button", { name: /已归档/ });
  await expect(filter).toBeVisible();

  await filter.click();
  const archivedCard = page.locator(".cron-card", { hasText: "归档的周报" });
  await expect(archivedCard).toHaveCount(1);
  await expect(archivedCard.locator(".cron-card-status")).toHaveText("已归档");

  await archivedCard.getByRole("button", { name: "更多操作" }).click();
  await page.getByRole("menuitem", { name: "恢复任务" }).click();

  await expect(archivedCard).toHaveCount(0);
  await expect
    .poll(() => callsFor(page, "archive_cron_job"))
    .toEqual([{ id: "archived-report", archived: false }]);
});

test("deleting a scheduled task asks how to handle session and run records", async ({
  page,
}) => {
  await page.goto(STORY);

  const card = page.locator(".cron-card", { hasText: "每日 AI 新闻推送" });
  await card.getByRole("button", { name: "更多操作" }).click();
  await page.getByRole("menuitem", { name: "删除任务" }).click();

  const dialog = page.locator(".app-dialog");
  await expect(dialog).toBeVisible();
  const archiveSession = dialog.getByRole("checkbox", {
    name: /同时归档它的执行会话/,
  });
  const deleteRuns = dialog.getByRole("checkbox", {
    name: /同时删除全部执行记录/,
  });
  // 默认只归档会话、不动运行记录
  await expect(archiveSession).toBeChecked();
  await expect(deleteRuns).not.toBeChecked();

  await deleteRuns.check();
  await dialog.getByRole("button", { name: "删除任务" }).click();
  await expect(dialog).toHaveCount(0);

  await expect
    .poll(() => callsFor(page, "remove_cron_job"))
    .toEqual([
      { args: { id: "daily-ai-news", archiveSession: true, deleteRuns: true } },
    ]);
  await expect(card).toHaveCount(0);
});

test("deleting without the session checkbox skips archiving it", async ({
  page,
}) => {
  await page.goto(STORY);

  const card = page.locator(".cron-card", { hasText: "每周工作周报" });
  await card.getByRole("button", { name: "更多操作" }).click();
  await page.getByRole("menuitem", { name: "删除任务" }).click();

  const dialog = page.locator(".app-dialog");
  await dialog
    .getByRole("checkbox", { name: /同时归档它的执行会话/ })
    .uncheck();
  await dialog.getByRole("button", { name: "删除任务" }).click();

  await expect
    .poll(() => callsFor(page, "remove_cron_job"))
    .toEqual([
      { args: { id: "weekly-report", archiveSession: false, deleteRuns: false } },
    ]);
});

test("sidebar labels cron sessions whose task is archived or deleted", async ({
  page,
}) => {
  await page.goto(SIDEBAR_STORY);

  const archivedRow = page.locator(".sidebar-session-item", {
    hasText: "归档的周报",
  });
  await expect(
    archivedRow.locator(".sidebar-session-cron-owner.is-archived"),
  ).toHaveText("任务已归档");

  const missingRow = page.locator(".sidebar-session-item", {
    hasText: "已删除的备份任务",
  });
  await expect(
    missingRow.locator(".sidebar-session-cron-owner.is-missing"),
  ).toHaveText("任务已删除");
  await expect(missingRow).toHaveClass(/is-cron-owner-missing/);

  // 仍在调度中的任务会话不加标注，避免噪音
  const activeRow = page.locator(".sidebar-session-item", {
    hasText: "每日 8 点舆情早报",
  });
  await expect(activeRow.locator(".sidebar-session-cron-owner")).toHaveCount(0);
});
