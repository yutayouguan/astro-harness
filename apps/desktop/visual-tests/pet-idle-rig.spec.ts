import { expect, test } from "@playwright/test";

test("hybrid idle uses matched layers, freezes in place and yields to APNG", async ({ page }) => {
  await page.goto("/iframe.html?id=desktop-pethybrid--layers&viewMode=story");
  for (const pet of ["naitang", "pudding"]) {
    await page.getByLabel("宠物", { exact: true }).selectOption(pet);
    const canvas = page.locator("canvas").first();
    await expect(canvas).toHaveAttribute("data-pet-renderer", "layered-idle");
    const pixels = () => canvas.evaluate((node: HTMLCanvasElement) => {
      const context = node.getContext("2d")!;
      const paws = context.getImageData(Math.round(node.width * 0.53), Math.round(node.height * 0.88), 20, 15).data;
      let pawsHash = 0; for (const byte of paws) pawsHash = (pawsHash * 31 + byte) | 0;
      return { all: node.toDataURL(), paws: pawsHash, corner: context.getImageData(0, 0, 10, 10).data.some((v, i) => i % 4 === 3 && v !== 0) };
    });
    const before = await pixels();
    await page.getByRole("button", { name: "看右侧" }).click();
    await expect.poll(async () => (await pixels()).all).not.toBe(before.all);
    expect((await pixels()).paws).toEqual(before.paws);
    expect((await pixels()).corner).toBe(false);
    await page.getByLabel("暂停", { exact: true }).check();
    await page.waitForTimeout(80);
    await page.screenshot({ path: `../../output/qa/hybrid-idle-${pet}-${test.info().project.name}.png` });
    const paused = await pixels();
    await page.getByRole("button", { name: "看左侧" }).click();
    await page.waitForTimeout(300);
    expect(await pixels()).toEqual(paused);
    await page.getByLabel("暂停", { exact: true }).uncheck();
    await expect.poll(async () => (await pixels()).all).not.toBe(paused.all);
    await page.getByLabel("减少动态").check();
    await page.waitForTimeout(100);
    const reduced = await pixels();
    await page.waitForTimeout(300);
    expect(await pixels()).toEqual(reduced);
    await page.getByLabel("减少动态").uncheck();
    await page.getByLabel("动作", { exact: true }).selectOption(pet === "naitang" ? "kneading" : "tail-wag");
    await expect(canvas).not.toHaveAttribute("data-pet-renderer", "layered-idle");
    await expect(page.getByRole("alert")).toHaveCount(0);
    await page.getByLabel("动作", { exact: true }).selectOption("idle");
    await expect(canvas).toHaveAttribute("data-pet-renderer", "layered-idle");
  }
  const canvas = page.locator("canvas").first();
  await page.getByLabel("不匹配素材").check();
  await expect(canvas).not.toHaveAttribute("data-pet-renderer", "layered-idle");
  await page.getByLabel("不匹配素材").uncheck();
  await expect(canvas).toHaveAttribute("data-pet-renderer", "layered-idle");
  await page.getByLabel("旧版待机图集").check();
  await expect(canvas).toHaveAttribute("data-pet-renderer", "layered-idle");
  await expect(canvas).toHaveJSProperty("width", 192);
  await page.getByLabel("宠物", { exact: true }).selectOption("naitang");
  await expect(canvas).toHaveAttribute("data-pet-renderer", "layered-idle");
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.waitForTimeout(100);
  const still = await canvas.evaluate((node: HTMLCanvasElement) => node.toDataURL());
  await page.getByRole("button", { name: "看右侧" }).click();
  await page.waitForTimeout(250);
  expect(await canvas.evaluate((node: HTMLCanvasElement) => node.toDataURL())).toBe(still);
});
