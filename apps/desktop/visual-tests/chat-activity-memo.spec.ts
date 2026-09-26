import { expect, test } from "@playwright/test";

// 流式 flush 只替换当前消息对象；历史活动行的 props 不变，
// memo 之后这些行不再渲染（渲染期间不会再次读取 props）。
test("unchanged activity rows skip re-render on a streaming flush", async ({
  page,
}) => {
  await page.goto(
    "/iframe.html?id=chat-activity-rows--streaming-flush&viewMode=story",
  );
  const rows = page.locator(".msg-activity-group");
  await expect(rows).toHaveCount(2);

  const reads = () => page.evaluate(() => window.__propReads ?? 0);

  const initial = await reads();
  expect(initial).toBeGreaterThan(0);

  for (const tick of ["1", "2", "3"]) {
    await page.getByRole("button", { name: "flush" }).click();
    await expect(page.getByTestId("tick")).toHaveText(tick);
  }

  expect(await reads()).toBe(initial);
});
