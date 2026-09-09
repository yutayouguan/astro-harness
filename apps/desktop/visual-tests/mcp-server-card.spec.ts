import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("astro-locale", "zh"));
});

for (const story of ["disabled", "compact-list"]) {
  for (const [width, theme] of [[1100, "light"], [420, "light"], [420, "dark"]] as const) {
    test(`${story} actions align at ${width}px in ${theme}`, async ({ page }, testInfo) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(`/iframe.html?id=settings-mcpservercard--${story}&viewMode=story&globals=theme:${theme}`);
      const reconnect = page.getByRole("button", { name: "重连", exact: true });
      const remove = page.getByRole("button", { name: "删除", exact: true });
      await expect(reconnect).toBeVisible();
      await expect(page.getByRole("img", { name: "运行状态: 已停用" })).toBeVisible();
      const first = (await reconnect.boundingBox())!;
      const second = (await remove.boundingBox())!;
      expect(first.height).toBe(32);
      expect(second.height).toBe(first.height);
      expect(second.y).toBe(first.y);
      expect(second.x - (first.x + first.width)).toBeCloseTo(8, 0);
      const styles = await page.locator(".mcp-server-action").evaluateAll(buttons => buttons.map(button => {
        const style = getComputedStyle(button);
        return [style.padding, style.borderRadius, style.borderWidth, style.lineHeight, style.fontWeight, style.backgroundImage, style.boxShadow];
      }));
      expect(styles[0]).toEqual(styles[1]);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
      await page.locator(".mcp-server-card").screenshot({ path: testInfo.outputPath(`${story}-${width}-${theme}.png`) });
    });
  }
}

test("every runtime state has a visible, labelled status light", async ({ page }) => {
  await page.goto("/iframe.html?id=settings-mcpservercard--all-states&viewMode=story");
  const states = ["configured", "disabled", "connecting", "connected", "disconnected", "backoff", "auth-required", "error", "unknown"];
  for (const state of states) {
    const light = page.locator(`[data-fixture-state="${state}"] .mcp-runtime-status`);
    await expect(light).toBeVisible();
    await expect(light).toHaveAttribute("data-status", state);
    await expect(light).toHaveAttribute("aria-label", /^运行状态: .+/);
    await expect(light).toHaveAttribute("title", await light.getAttribute("aria-label") as string);
  }
  const color = (state: string) => page.locator(`[data-fixture-state="${state}"] .mcp-runtime-status-dot`).evaluate(el => getComputedStyle(el).backgroundColor);
  expect(await color("connected")).not.toBe(await color("error"));
  expect(await color("connected")).not.toBe(await color("connecting"));
});

test("reconnect preserves button layout while pending", async ({ page }) => {
  await page.goto("/iframe.html?id=settings-mcpservercard--connected&viewMode=story");
  await page.getByRole("button", { name: "重连", exact: true }).click();
  await expect(page.getByRole("button", { name: "重连中…", exact: true })).toBeDisabled();
  await expect(page.getByRole("img", { name: "运行状态: 连接中" })).toBeVisible();
  const boxes = await page.locator(".mcp-server-action").evaluateAll(buttons => buttons.map(button => ({ y: button.getBoundingClientRect().y, height: button.getBoundingClientRect().height })));
  expect(boxes[0]).toEqual(boxes[1]);
});
