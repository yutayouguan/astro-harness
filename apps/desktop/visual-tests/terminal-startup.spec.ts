import { expect, test } from "@playwright/test";

for (const [fontSize, story] of [[13, "narrow"], [20, "split-output"]] as const) {
test(`zsh startup stays on row zero at font size ${fontSize} (${story})`, async ({ page }) => {
  await page.setViewportSize({ width: 520, height: 500 });
  await page.addInitScript((size) => localStorage.setItem("astro.terminal.settings.v1", JSON.stringify({ fontSize: size })), fontSize);
  await page.goto(`/iframe.html?id=chat-terminalstartup--${story}&viewMode=story`);
  await expect(page.locator(".xterm")).toBeVisible();
  const screen = () => page.evaluate(() => (window as any).terminalStartupPreview.screen());
  await expect.poll(async () => (await screen()).lines.join("\n")).toContain("AI workspace %");
  await expect.poll(async () => (await screen()).lines[0]?.trimEnd()).toBe("AI workspace %");
  expect((await screen()).cursorY).toBe(0);
  const requests = await page.evaluate(() => (window as any).terminalStartupPreview.requests);
  expect(requests.find((r: any) => r.command === "terminal_open").cols).toBe((await screen()).cols);
  expect(requests.findIndex((r: any) => r.command === "terminal_resize")).toBeGreaterThanOrEqual(0);
  expect(requests.findIndex((r: any) => r.command === "terminal_resize")).toBeLessThan(requests.findIndex((r: any) => r.command === "terminal_read"));
  await page.screenshot({ path: `test-results/terminal-startup-${fontSize}.png` });
  await page.setViewportSize({ width: 900, height: 500 });
  await expect.poll(async () => (await screen()).cols).toBeGreaterThan(requests[0].cols);
  await expect.poll(async () => {
    const data = await page.evaluate(() => (window as any).terminalStartupPreview.requests);
    return data.filter((r: any) => r.command === "terminal_resize").at(-1)?.cols;
  }).toBe((await screen()).cols);
  await page.locator(".terminal-tab-add").click();
  await expect.poll(async () => (await screen()).lines[0]?.trimEnd()).toBe("AI workspace %");
  expect((await screen()).cursorY).toBe(0);
  const reopened = await page.evaluate(() => (window as any).terminalStartupPreview.requests.filter((r: any) => r.command === "terminal_open"));
  expect(reopened).toHaveLength(2);
  expect(reopened[1].cols).toBe((await screen()).cols);
});
}

test("prefill request opens a tab whose shell gets the command without running it", async ({
  page,
}) => {
  await page.setViewportSize({ width: 720, height: 500 });
  await page.goto(`/iframe.html?id=chat-terminalstartup--prefilled&viewMode=story`);
  await expect(page.locator(".xterm")).toBeVisible();

  const opened = await page.evaluate(() =>
    (window as any).terminalStartupPreview.requests.filter(
      (r: any) => r.command === "terminal_open",
    ),
  );
  // 预填只随首次打开下发：命令只是被写进 shell，不追加换行（不执行）。
  expect(
    opened.some((r: any) => r.initialInput === "cat README.md"),
  ).toBeTruthy();
  expect(
    opened.filter((r: any) => r.initialInput === "cat README.md"),
  ).toHaveLength(1);
});
