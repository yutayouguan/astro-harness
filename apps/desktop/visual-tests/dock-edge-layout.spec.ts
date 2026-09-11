import { expect, test } from "@playwright/test";

for (const theme of ["light", "dark"]) {
  for (const [story, panelSelector, headerSelector] of [
    ["project-files", ".project-files-panel.is-open", ".project-files-header"],
    ["side-chat", ".side-chat-panel", ".side-chat-head"],
    ["runtime-panel", ".chat-right-panel", ".chat-right-header"],
  ]) {
    test(`${story} headings sit just below the titlebar in ${theme}`, async ({ page }) => {
      await page.setViewportSize({ width: 1280, height: 800 });
      await page.goto(`/iframe.html?id=chat-dock-layout--${story}&viewMode=story&globals=theme:${theme}`);
      const header = page.locator(headerSelector);
      await expect(header).toBeVisible();
      const globalHeader = await page.locator(".content-header--chat").boundingBox();
      const local = await header.boundingBox();
      const panel = await page.locator(panelSelector).boundingBox();
      const layout = await page.locator(".chat-layout-with-right").boundingBox();
      expect(local!.y - (globalHeader!.y + globalHeader!.height)).toBeGreaterThanOrEqual(4);
      expect(local!.y - (globalHeader!.y + globalHeader!.height)).toBeLessThanOrEqual(5);
      expect(Math.abs(panel!.y - layout!.y)).toBeLessThanOrEqual(1);
      // Actual hit testing protects close/refresh actions from the global drag layer.
      for (const button of await header.locator("button").all()) {
        await button.click({ trial: true });
      }
      await header.screenshot({ path: `test-results/dock-header-${story}-${theme}.png` });
      await page.screenshot({
        path: `test-results/dock-chrome-${story}-${theme}.png`,
        clip: { x: 680, y: 0, width: 600, height: 200 },
      });
    });
  }
}

test("browser native viewport fills both edges while expand/restore and resize remain usable", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/iframe.html?id=chat-dock-layout--browser&viewMode=story");
  const dock = page.locator(".browser-dock");
  const viewport = page.locator(".browser-native-viewport");
  await expect(dock).toHaveClass(/is-open/);
  const checkEdges = async () => {
    const outer = await dock.boundingBox();
    const inner = await viewport.boundingBox();
    expect(Math.abs(inner!.x - outer!.x)).toBeLessThanOrEqual(1);
    expect(Math.abs(inner!.x + inner!.width - outer!.x - outer!.width)).toBeLessThanOrEqual(1);
  };
  await checkEdges();
  const resize = dock.getByRole("separator");
  const grip = await resize.boundingBox();
  const native = await viewport.boundingBox();
  expect(grip!.y + grip!.height).toBeLessThanOrEqual(native!.y);
  await resize.click({ trial: true });
  const before = Number(await resize.getAttribute("aria-valuenow"));
  await resize.focus();
  await resize.press("ArrowLeft");
  await expect(resize).not.toHaveAttribute("aria-valuenow", String(before));
  await expect.poll(async () => (await dock.boundingBox())!.width).toBeCloseTo(Number(await resize.getAttribute("aria-valuenow")), 0);
  await checkEdges();
  const pointerGrip = await resize.boundingBox();
  const pointerWidth = Number(await resize.getAttribute("aria-valuenow"));
  await page.mouse.move(pointerGrip!.x + 6, pointerGrip!.y + 17);
  await page.mouse.down();
  await page.mouse.move(pointerGrip!.x + 46, pointerGrip!.y + 17, { steps: 4 });
  await page.mouse.up();
  await expect(resize).not.toHaveAttribute("aria-valuenow", String(pointerWidth));
  await checkEdges();
  const expand = dock.locator(".browser-dock-expand");
  await expand.click();
  await expect(dock).toHaveClass(/is-expanded/);
  await expect(resize).toHaveCount(0);
  const layout = await page.locator(".chat-layout-with-right").boundingBox();
  await expect.poll(async () => (await dock.boundingBox())!.width).toBeCloseTo(layout!.width, 0);
  await checkEdges();
  await expand.click();
  await expect(dock).not.toHaveClass(/is-restoring|is-expanded/);
  await checkEdges();
  await page.setViewportSize({ width: 760, height: 800 });
  await expect.poll(async () => (await dock.boundingBox())!.width).toBeLessThanOrEqual(760);
  await checkEdges();
});
