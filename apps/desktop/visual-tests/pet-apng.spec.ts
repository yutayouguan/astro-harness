import { expect, test } from "@playwright/test";

test("APNG actions decode on both engines, stay transparent and respect reduced motion", async ({ page }) => {
  await page.goto("/iframe.html?id=desktop-petapng--actions&viewMode=story");
  const canvas = page.locator("canvas").first();
  const signature = () => canvas.evaluate((node: HTMLCanvasElement) => {
    const bytes = node.getContext("2d")!.getImageData(0, 0, node.width, node.height).data;
    let visible = 0, transparent = 0, hash = 0;
    for (let i = 0; i < bytes.length; i++) { hash = (hash * 31 + bytes[i]) | 0;
      if (i % 4 === 3) { if (bytes[i]) visible++; else transparent++; } }
    return { hash, visible, transparent };
  });
  for (const pet of ["naitang", "pudding"]) {
    await page.getByLabel("APNG宠物").selectOption(pet);
    const actions = await page.getByLabel("APNG动作").locator("option").allTextContents();
    for (const action of actions) {
      await page.getByLabel("APNG动作").selectOption(action);
      await expect.poll(async () => (await signature()).visible).toBeGreaterThan(1000);
      expect((await signature()).transparent).toBeGreaterThan(1000);
      await expect(page.getByRole("alert")).toHaveCount(0);
    }
  }
  await page.getByLabel("APNG动作").selectOption("tail-wag");
  await expect.poll(async () => (await signature()).visible).toBeGreaterThan(1000);
  const before = await signature();
  await expect.poll(async () => (await signature()).hash).not.toBe(before.hash);
  await page.getByLabel("减少动态").check();
  await expect.poll(async () => (await signature()).visible).toBeGreaterThan(1000);
  const still = await signature();
  await page.waitForTimeout(350);
  expect(await signature()).toEqual(still);
});
