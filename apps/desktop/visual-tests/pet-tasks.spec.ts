import { test, expect } from "@playwright/test";
test("capsule morphs in place, reverses mid-flight and retains the same draft", async ({
  page,
}) => {
  await page.goto("/iframe.html?id=desktop-pettasks--badge&viewMode=story");
  const shell = page.locator(".pet-task-morph");
  await expect(shell).toHaveCSS("width", "166px");
  await page.evaluate(() => {
    const samples: number[] = [];
    (window as unknown as { morphSamples: number[] }).morphSamples = samples;
    const sample = () => {
      const el = document.querySelector(".pet-task-morph");
      if (el) samples.push(el.getBoundingClientRect().width);
      if (samples.length < 90) requestAnimationFrame(sample);
    };
    requestAnimationFrame(sample);
  });
  await page.getByRole("button", { name: "待处理 2", exact: true }).click();
  await expect(shell).toHaveCSS("width", "392px");
  const intermediate = await page.evaluate(() =>
    (window as unknown as { morphSamples: number[] }).morphSamples.filter(
      (w) => w > 167 && w < 391,
    ),
  );
  expect(intermediate.length).toBeGreaterThan(2);
  await page.locator(".pet-task-row").filter({ hasText: "待回答" }).click();
  await page
    .getByRole("textbox", { name: "备注", exact: true })
    .fill("变形时也不能丢失");
  await page.getByRole("button", { name: "稍后处理", exact: true }).click();
  await page
    .getByRole("button", { name: "待处理 2", exact: true })
    .click({ force: true });
  expect((await shell.boundingBox())!.width).toBeGreaterThan(200);
  await expect(shell).toHaveCSS("width", "392px");
  await page.locator(".pet-task-row").filter({ hasText: "待回答" }).click();
  await expect(
    page.getByRole("textbox", { name: "备注", exact: true }),
  ).toHaveValue("变形时也不能丢失");
  await shell.screenshot({
    path: test.info().outputPath("pet-morph-expanded.png"),
  });
  await page.getByRole("button", { name: "稍后处理", exact: true }).click();
  await expect(shell).toHaveCSS("width", "166px");
  await expect(shell).toHaveCSS("height", "38px");
});
test("reduced motion switches the shared glass surface without spatial animation", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/iframe.html?id=desktop-pettasks--badge&viewMode=story");
  await page.getByRole("button", { name: "待处理 2", exact: true }).click();
  await expect(page.locator(".pet-task-morph")).toHaveCSS("width", "392px");
  await page.getByRole("button", { name: "稍后处理", exact: true }).click();
  await expect(page.locator(".pet-task-morph")).toHaveCSS("width", "166px");
});
test("task glass has an opaque high-contrast fallback", async ({ page }) => {
  await page.emulateMedia({ contrast: "more" });
  await page.goto(
    "/iframe.html?id=desktop-pettasks--retry-question&viewMode=story&globals=theme:light",
  );
  await expect(page.locator(".pet-task-morph")).toHaveCSS(
    "backdrop-filter",
    "none",
  );
});
for (const theme of ["light", "dark"]) {
  test(`task badge is a frosted capsule in ${theme}`, async ({ page }) => {
    await page.goto(
      `/iframe.html?id=desktop-pettasks--badge&viewMode=story&globals=theme:${theme}`,
    );
    const badge = page.getByRole("button", { name: "待处理 2", exact: true });
    const shell = page.locator(".pet-task-morph");
    await expect(shell).toHaveCSS("border-radius", "19px");
    await expect(shell).toHaveCSS(
      "backdrop-filter",
      "blur(28px) saturate(1.5)",
    );
    const bounds = await shell.boundingBox();
    expect(bounds?.width).toBe(166);
    expect(bounds?.height).toBe(38);
    await expect(badge).toBeVisible();
    await shell.screenshot({
      path: test.info().outputPath(`pet-badge-glass-${theme}.png`),
    });
  });
  test(`task popup has rounded glass and clear primary action in ${theme}`, async ({
    page,
  }) => {
    await page.goto(
      `/iframe.html?id=desktop-pettasks--retry-question&viewMode=story&globals=theme:${theme}`,
    );
    const popup = page.locator(".pet-task-morph");
    await expect(popup).toHaveCSS("border-radius", "19px");
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
