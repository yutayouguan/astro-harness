import { expect, test } from "@playwright/test";

test("applying a scene preserves the list layout and decoded previews", async ({ page }) => {
  await page.setViewportSize({ width: 1180, height: 1000 });
  await page.goto("/iframe.html?id=settings-petscenestudio--preview-and-failure&viewMode=story");
  await page.getByRole("button", { name: "管理 奶糖", exact: true }).click();
  const list = page.locator(".pet-scene-list");
  await expect(list.locator(".pet-scene-card")).toHaveCount(2);
  await expect(page.getByText("正在读取场景…", { exact: true })).toHaveCount(0);
  await expect(list.locator(".pet-scene-wallpaper")).toHaveJSProperty("complete", true);
  await list.scrollIntoViewIfNeeded();

  await page.evaluate(() => {
    const win = window as typeof window & {
      __TAURI_INTERNALS__: { invoke: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown> };
      __sceneApplyProbe?: {
        releaseApply?: () => void;
        releaseRefresh?: () => void;
        list: Element;
        previews: Element[];
        heights: number[];
        offsets: number[];
        stop?: () => void;
      };
    };
    const list = document.querySelector(".pet-scene-list")!;
    const section = list.closest(".pet-scenes")!;
    const probe = win.__sceneApplyProbe = {
      list, previews: [...list.querySelectorAll("canvas, img")],
      heights: [] as number[], offsets: [] as number[],
    } as NonNullable<typeof win.__sceneApplyProbe>;
    let sampling = true;
    const sample = () => {
      probe.heights.push(section.getBoundingClientRect().height);
      probe.offsets.push(list.getBoundingClientRect().top - section.getBoundingClientRect().top);
      if (sampling) requestAnimationFrame(sample);
    };
    probe.stop = () => { sampling = false; };
    sample();
    const invoke = win.__TAURI_INTERNALS__.invoke;
    win.__TAURI_INTERNALS__.invoke = async (command, args, options) => {
      if (command === "apply_pet_scene")
        await new Promise<void>((resolve) => { probe.releaseApply = resolve; });
      if (command === "get_pet_scenes")
        await new Promise<void>((resolve) => { probe.releaseRefresh = resolve; });
      return invoke(command, args, options);
    };
  });
  const apply = list.getByRole("button", { name: "应用整套", exact: true }).last();
  await apply.click();
  await expect(apply).toBeDisabled();
  await page.evaluate(() => new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  ));
  await expect.poll(() => page.evaluate(() => {
    const probe = (window as any).__sceneApplyProbe;
    return Math.max(...probe.offsets) - Math.min(...probe.offsets);
  })).toBeLessThan(1);
  await page.evaluate(() => (window as any).__sceneApplyProbe.releaseApply());
  await expect.poll(() => page.evaluate(() => !!(window as any).__sceneApplyProbe.releaseRefresh)).toBe(true);
  await expect(apply).toBeEnabled();
  await page.evaluate(() => new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  ));
  await expect.poll(() => page.evaluate(() => {
    const probe = (window as any).__sceneApplyProbe;
    return Math.max(...probe.heights) - Math.min(...probe.heights);
  })).toBeLessThan(1);
  await page.evaluate(() => (window as any).__sceneApplyProbe.releaseRefresh());
  await expect(list.locator(".pet-scene-active")).toHaveCount(1);
  await expect(list.locator(".pet-scene-card").last().locator(".pet-scene-active")).toBeVisible();
  await page.evaluate(() => new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  ));
  expect(await page.evaluate(() => {
    const probe = (window as any).__sceneApplyProbe;
    probe.stop();
    return Math.max(...probe.heights) - Math.min(...probe.heights) < 1 &&
      Math.max(...probe.offsets) - Math.min(...probe.offsets) < 1 &&
      probe.list === document.querySelector(".pet-scene-list") &&
      probe.previews.every((node: Element) => node.isConnected);
  })).toBe(true);
});
