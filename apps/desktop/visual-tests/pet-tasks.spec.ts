import { test, expect } from "@playwright/test";
test("task glass has an opaque high-contrast fallback", async ({ page }) => {
  await page.emulateMedia({ contrast: "more" });
  await page.goto(
    "/iframe.html?id=desktop-pettasks--retry-question&viewMode=story&globals=theme:light",
  );
  await expect(page.locator(".pet-task-popup")).toHaveCSS(
    "backdrop-filter",
    "none",
  );
});
for (const theme of ["light", "dark"]) {
  test(`task popup has rounded glass and clear primary action in ${theme}`, async ({
    page,
  }) => {
    await page.goto(
      `/iframe.html?id=desktop-pettasks--retry-question&viewMode=story&globals=theme:${theme}`,
    );
    const popup = page.locator(".pet-task-popup");
    await expect(popup).toHaveCSS("border-radius", "24px");
    await expect(popup).toHaveCSS(
      "backdrop-filter",
      "blur(28px) saturate(1.5)",
    );
    await expect(
      page.getByRole("button", { name: "提交回答", exact: true }),
    ).toHaveCSS("color", "rgb(255, 255, 255)");
    await expect
      .poll(async () => (await popup.boundingBox())?.height ?? 1000)
      .toBeLessThan(520);
    await popup.screenshot({
      path: test.info().outputPath(`pet-popup-glass-${theme}.png`),
    });
    await page
      .getByRole("button", { name: "返回任务列表", exact: true })
      .click();
    await popup.screenshot({
      path: test.info().outputPath(`pet-task-list-${theme}.png`),
    });
  });
}
test("an already-focused popup input allows clicking to reposition the caret", async ({
  page,
}) => {
  await page.goto(
    "/iframe.html?id=desktop-pettasks--retry-question&viewMode=story",
  );
  const note = page.getByRole("textbox", { name: "备注", exact: true });
  await note.fill("alpha beta gamma");
  await expect(note).toBeFocused();
  await note.click({ position: { x: 12, y: 12 } });
  await expect
    .poll(() =>
      note.evaluate((input: HTMLInputElement) => input.selectionStart),
    )
    .toBeLessThan(5);
});
test("multi-step original questions retain answers after a failed submission", async ({
  page,
}) => {
  await page.goto(
    "/iframe.html?id=desktop-pettasks--multi-step-question&viewMode=story",
  );
  await page.getByRole("button", { name: /^本地/ }).click();
  await expect(
    page.getByText("原始问题二：请补充说明", { exact: true }),
  ).toBeVisible();
  await page
    .getByRole("textbox", { name: "补充其他内容" })
    .fill("多步草稿不能丢失");
  await page.getByRole("button", { name: "提交", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("模拟网络失败");
  await expect(page.getByRole("textbox", { name: "补充其他内容" })).toHaveValue(
    "多步草稿不能丢失",
  );
  await page.getByRole("tab").first().click();
  await expect(page.getByRole("button", { name: /^本地/ })).toHaveClass(
    /is-selected/,
  );
  await page.getByRole("tab").nth(1).click();
  await page.getByRole("button", { name: "提交", exact: true }).click();
  await expect(page.getByText("请完成两步确认", { exact: true })).toHaveCount(
    0,
  );
});
test("failed question submission preserves the original answer and can be retried", async ({
  page,
}) => {
  await page.goto(
    "/iframe.html?id=desktop-pettasks--retry-question&viewMode=story&globals=theme:dark",
  );
  await page.getByLabel("部署环境 *").selectOption("1");
  await page.getByLabel("备注").fill("不能丢失的输入");
  await page.getByRole("button", { name: "提交回答", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("模拟网络失败");
  await expect(page.getByLabel("备注")).toHaveValue("不能丢失的输入");
  await expect(page.getByLabel("部署环境 *")).toHaveValue("1");
  await page.screenshot({
    path: test.info().outputPath("pet-task-retry-dark.png"),
  });
  await page.getByRole("button", { name: "提交回答", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "提交回答", exact: true }),
  ).toHaveCount(0);
});
test("pet approvals require explicit persistent confirmation and keep sibling question", async ({
  page,
}) => {
  await page.goto(
    "/iframe.html?id=desktop-pettasks--approval-and-question&viewMode=story",
  );
  await expect(page.getByText("确认待执行操作")).toBeVisible();
  await page
    .getByRole("button", { name: "永久允许此操作", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "确认永久允许此操作", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "仅本次允许", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "仅本次允许", exact: true }).click();
  await expect(page.getByText("请选择部署环境", { exact: true })).toBeVisible();
  const select = page.getByLabel("部署环境 *");
  await expect(select).toHaveValue("");
  await select.selectOption("0");
  await select.selectOption("");
  await expect(select).toHaveValue("");
  await page.getByLabel("备注").fill("保留的草稿");
  await page.getByRole("button", { name: "返回任务列表", exact: true }).click();
  await page.locator(".pet-task-row").filter({ hasText: "待回答" }).click();
  await expect(page.getByLabel("备注")).toHaveValue("保留的草稿");
  await page.screenshot({
    path: test.info().outputPath("pet-task-question.png"),
  });
  await select.selectOption("0");
  await page.getByRole("button", { name: "提交回答", exact: true }).click();
  await expect(page.getByText("请选择部署环境", { exact: true })).toHaveCount(
    0,
  );
});
