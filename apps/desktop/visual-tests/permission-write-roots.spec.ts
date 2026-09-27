import { expect, test } from "@playwright/test";

// 权限设置页的「永久可写目录」：列出现有根、可增删，并能把本会话授权固化下来。
const STORY =
  "/iframe.html?id=tools-approvals--default&viewMode=story&globals=theme:light";

test("permanent writable folders can be edited and promoted from the approvals panel", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 1000 });
  await page.goto(STORY);

  const card = page.locator(".approvals-write-roots-card");
  await expect(card).toBeVisible({ timeout: 30_000 });
  const roots = card.locator(".approvals-allow-list .mcp-tool-name");
  await expect(roots).toHaveText(["/tmp/qa-shared-out"]);

  // 新增：绝对路径写入成功并进入列表。
  // 相对路径在提交前就被挡下（按钮禁用 + 即时提示）。
  const input = card.getByPlaceholder("如：/Users/me/shared-out（绝对路径）");
  await input.fill("relative/out");
  await expect(card).toContainText("请输入绝对路径");
  await expect(card.getByRole("button", { name: "添加" })).toBeDisabled();

  await card
    .getByPlaceholder("如：/Users/me/shared-out（绝对路径）")
    .fill("/tmp/qa-added-out");
  await card.getByRole("button", { name: "添加" }).click();
  await expect(roots).toHaveText(["/tmp/qa-shared-out", "/tmp/qa-added-out"]);

  // 移除：只删掉目标行。
  await card
    .locator(".mcp-tool-row", { hasText: "/tmp/qa-added-out" })
    .getByRole("button", { name: "移除" })
    .click();
  await expect(roots).toHaveText(["/tmp/qa-shared-out"]);

  // 固化本会话授权：本会话还有 1 项额外权限，写入后列表变长并给出提示。
  const promote = card.getByRole("button", {
    name: "把本会话额外权限写入永久",
  });
  await expect(card).toContainText("本会话还有 1 项额外权限");
  await promote.click();
  await expect(roots).toHaveText(["/tmp/qa-shared-out", "/tmp/qa-session-out"]);
  await expect(card).toContainText("已写入永久可写目录。");
  await expect(promote).toBeDisabled();
});
