import { expect, test, type Page } from "@playwright/test";

// 授权/批准共用同一张卡：确认（工具批准、沙箱提权重试）与网络授权只是动作作用域不同。
// 这里锁定外观契约（分类色 + 两行对齐的动作区 + 终端面板）与动作结果文案。
async function openApprovalStory(page: Page, id: string) {
  await page.setViewportSize({ width: 1000, height: 900 });
  await page.goto(`/iframe.html?id=${id}&viewMode=story&globals=theme:light`);
  await expect(page.getByTestId("clarify-wizard-preview")).toBeVisible({
    timeout: 30000,
  });
  const card = page.locator(".a2ui-clarify-wizard.is-approval");
  await expect(card).toHaveCount(1);
  return card;
}

test("tool approval card keeps a terminal panel and aligned action rows", async ({
  page,
}) => {
  const card = await openApprovalStory(page, "clarify-wizard--approval");

  await expect(card.locator(".a2ui-approval-eyebrow")).toHaveText("需要批准");
  await expect(card.locator(".a2ui-approval-title")).toHaveText("批准危险命令");
  await expect(card.locator(".a2ui-approval-actions button")).toHaveCount(2);
  await expect(card.locator(".a2ui-approval-copy")).toHaveCount(1);
  await expect(card.locator(".a2ui-approval-always")).toHaveCount(2);

  // 命令块保留原始换行（不再中途折行），交给横向滚动。
  await expect(card.locator(".a2ui-approval-command pre")).toHaveCSS(
    "white-space",
    "pre",
  );

  // 主决策与长期授权两行左右对齐。
  const deny = await card.locator(".a2ui-approval-action.is-deny").boundingBox();
  const always = await card
    .locator(".a2ui-approval-always")
    .first()
    .boundingBox();
  expect(Math.abs((deny?.x ?? 0) - (always?.x ?? 0))).toBeLessThanOrEqual(1);
  const approve = await card
    .locator(".a2ui-approval-action.is-approve")
    .boundingBox();
  const typeRule = await card
    .locator(".a2ui-approval-always.is-type")
    .boundingBox();
  expect(Math.abs((approve?.x ?? 0) - (typeRule?.x ?? 0))).toBeLessThanOrEqual(1);

  // 批准一次 → 折叠为结果行。
  await card.locator(".a2ui-approval-action.is-approve").click();
  await expect(page.locator(".a2ui-approval-result")).toHaveText(
    "已批准本次操作",
  );
});

test("network authorization card reuses the same surface with scoped actions", async ({
  page,
}) => {
  const card = await openApprovalStory(
    page,
    "clarify-wizard--approval-network",
  );

  await expect(card.locator(".a2ui-approval-eyebrow")).toHaveText("需要授权");
  await expect(card.locator(".a2ui-approval-title")).toHaveText("网络访问");
  await expect(card.locator(".a2ui-approval-command-label")).toContainText(
    "目标地址",
  );
  await expect(card.locator(".a2ui-approval-command code")).toHaveText(
    "https://api.virxact.com:8443",
  );
  await expect(card.locator(".a2ui-approval-description")).toContainText(
    "Profile：aihot",
  );
  await expect(card.locator(".a2ui-approval-action.is-approve")).toContainText(
    "仅本次允许",
  );

  const persistent = card.locator(".a2ui-approval-persistent-actions button");
  await expect(persistent).toHaveCount(2);
  await expect(persistent.nth(0)).toContainText("本次会话允许");
  await expect(persistent.nth(1)).toContainText("始终允许 api.virxact.com");

  // 会话级授权 → 折叠为结果行。
  await persistent.nth(0).click();
  await expect(page.locator(".a2ui-approval-result")).toHaveText(
    "已允许到本会话结束",
  );
});

test("sandbox retry authorization stays a one-shot decision", async ({
  page,
}) => {
  const card = await openApprovalStory(
    page,
    "clarify-wizard--approval-sandbox-retry",
  );

  await expect(card.locator(".a2ui-approval-eyebrow")).toHaveText("需要授权");
  await expect(card.locator(".a2ui-approval-title")).toHaveText("在沙箱外重试");
  // 有被拒命令时命令块放命令本身，拒绝详情进描述。
  await expect(card.locator(".a2ui-approval-command-label")).toContainText(
    "待执行命令",
  );
  await expect(card.locator(".a2ui-approval-description")).toContainText(
    "sandbox denied write to /Users/me/project/out/report.md",
  );
  // 只授予本次：没有长期授权行。
  await expect(card.locator(".a2ui-approval-actions button")).toHaveCount(2);
  await expect(card.locator(".a2ui-approval-persistent-actions")).toHaveCount(0);

  // 被拒命令要能看见，并且能一键预填到终端（不执行、也不算回答请求）。
  await expect(card.locator(".a2ui-approval-command code")).toContainText(
    "cat /Users/me/project/in/report.md",
  );
  const openInTerminal = card.getByRole("button", {
    name: /在终端打开/,
  });
  await expect(openInTerminal).toBeVisible();
  await openInTerminal.click();
  // 卡片保持打开：这是旁路动作，不是批准/拒绝。
  await expect(card).toBeVisible();
  await expect(page.locator(".a2ui-approval-result")).toHaveCount(0);
});

test("dangerous approvals show a risk label and require a second confirmation", async ({
  page,
}) => {
  const card = await openApprovalStory(
    page,
    "clarify-wizard--approval-dangerous",
  );

  await expect(card.locator(".a2ui-approval-risk")).toHaveText("高风险");
  // 第一次点击只落到内联确认，不会被当成批准。
  await card.locator(".a2ui-approval-action.is-approve").click();
  await expect(card.locator(".a2ui-approval-second-thoughts")).toBeVisible();
  await expect(page.locator(".a2ui-approval-result")).toHaveCount(0);

  // 返回后可重新选择；再次批准需要显式确认。
  await card.getByRole("button", { name: "返回", exact: true }).click();
  await expect(card.locator(".a2ui-approval-second-thoughts")).toHaveCount(0);
  await card.locator(".a2ui-approval-action.is-approve").click();
  await card.getByRole("button", { name: "确认批准" }).click();
  await expect(page.locator(".a2ui-approval-result")).toHaveText(
    "已批准本次操作",
  );
});
