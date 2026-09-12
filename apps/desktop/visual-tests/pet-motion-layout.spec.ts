import { expect, test } from "@playwright/test";

for (const width of [1280, 800, 430]) {
  test(`motion settings keep safe card insets at ${width}px`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto("/iframe.html?id=settings-petscenestudio--preview-and-failure&viewMode=story");
    await page.getByRole("button", { name: "管理 奶糖", exact: true }).click();
    await page.getByRole("tab", { name: "动作与偏好", exact: true }).click();
    const preferences = page.locator(".pet-motion-preferences");
    const measure = () => preferences.evaluate((el) => {
      const rect = el.parentElement!.getBoundingClientRect();
      const preview = document.querySelector(".pet-motion-preview")!.getBoundingClientRect();
      const save = el.querySelector(".pet-detail-save-row button")!.getBoundingClientRect();
      const header = el.querySelector("header")!.getBoundingClientRect();
      const switches = [...el.querySelectorAll('[role="switch"]')].map((node) => node.getBoundingClientRect());
      const wrapper = el.parentElement!;
      return {
        leftInset: header.left - rect.left,
        rightInset: Math.min(...switches.map((r) => rect.right - r.right), rect.right - save.right),
        bottomInset: rect.bottom - save.bottom,
        previewWidth: preview.width,
        aligned: Math.abs(preview.top - header.top) < 1,
        stacked: header.top >= preview.bottom,
        overflow: wrapper.scrollWidth > wrapper.clientWidth + 1,
        background: getComputedStyle(wrapper).backgroundColor,
      };
    });
    const bounds = await measure();
    expect(bounds.leftInset).toBeGreaterThanOrEqual(16);
    expect(bounds.rightInset).toBeGreaterThanOrEqual(16);
    expect(bounds.bottomInset).toBeGreaterThanOrEqual(16);
    expect(bounds.overflow).toBe(false);
    expect(bounds.background).not.toBe("rgba(0, 0, 0, 0)");
    if (width > 800) {
      expect(bounds.previewWidth).toBeLessThanOrEqual(340);
      expect(bounds.aligned).toBe(true);
    } else expect(bounds.stacked).toBe(true);
    // Pointer and keyboard interaction must still update the draft scale.
    const slider = page.getByRole("slider", { name: /桌宠大小/ });
    await slider.scrollIntoViewIfNeeded();
    const old = await slider.inputValue();
    const box = (await slider.boundingBox())!;
    await page.mouse.move(box.x + box.width * 0.4, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width * 0.9, box.y + box.height / 2, { steps: 8 });
    await page.mouse.up();
    expect(await slider.inputValue()).not.toBe(old);
    await expect(page.getByRole("button", { name: "保存配置", exact: true })).toBeEnabled();
    await page.getByRole("button", { name: "保存配置", exact: true }).scrollIntoViewIfNeeded();
    await page.screenshot({ path: testInfo.outputPath("motion-settings.png") });
  });
}

test("live resizing preserves the scene and other pets' saved defaults", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto("/iframe.html?id=settings-petscenestudio--preview-and-failure&viewMode=story");
  await expect(page.getByRole("button", { name: "管理 奶糖", exact: true })).toBeVisible();
  const snapshot = () => page.evaluate(async () => {
    const state = await (window as any).__TAURI_INTERNALS__.invoke("get_desktop_pet_state");
    return { scale: state.scale, petId: state.activePetId, sceneId: state.activeSceneId,
      followWallpaper: state.followWallpaper, defaults: state.pets.map((p: any) => ({ id: p.id, scale: p.defaults.scale })) };
  });
  const before = await snapshot();
  await page.getByRole("button", { name: "管理 奶糖", exact: true }).click();
  await page.getByRole("tab", { name: "动作与偏好", exact: true }).click();
  const slider = page.getByRole("slider", { name: /桌宠大小/ });
  await slider.press("End");
  await expect.poll(async () => (await snapshot()).scale).toBe(0.3);
  await expect(slider).toHaveAttribute("aria-valuetext", "100%");
  expect(await snapshot()).toEqual({ ...before, scale: 0.3 });
  await slider.press("Home");
  await expect.poll(async () => (await snapshot()).scale).toBe(0.15);
  await expect(slider).toHaveAttribute("aria-valuetext", "50%");
  expect(await snapshot()).toEqual({ ...before, scale: 0.15 });
  await page.locator(".pet-library-back").click();
  await page.getByRole("button", { name: "管理 布丁", exact: true }).click();
  await page.getByRole("tab", { name: "动作与偏好", exact: true }).click();
  await slider.press("Home");
  await expect(slider).toHaveValue("0.15");
  expect(await snapshot()).toEqual({ ...before, scale: 0.15 });
  await page.getByRole("button", { name: "保存配置", exact: true }).click();
  await expect.poll(async () => (await snapshot()).defaults.find((p: any) => p.id === "builtin-pudding")?.scale).toBe(0.15);
  expect((await snapshot()).scale).toBe(0.15);
  expect((await snapshot()).sceneId).toBe(before.sceneId);
});

test("rebased size labels reach 50 percent and agree with inherited scene cards", async ({ page }) => {
  await page.goto("/iframe.html?id=settings-petscenestudio--preview-and-failure&viewMode=story");
  await page.getByRole("button", { name: "管理 奶糖", exact: true }).click();
  await expect(page.locator(".pet-scene-card").first()).toContainText("100%");
  await page.getByRole("tab", { name: "动作与偏好", exact: true }).click();
  const slider = page.getByRole("slider", { name: /桌宠大小/ });
  await expect(slider).toHaveValue("0.3");
  await expect(slider).toHaveAttribute("max", "0.3");
  await expect(slider).toHaveAttribute("aria-valuetext", "100%");
  await slider.press("Home");
  await expect(slider).toHaveValue("0.15");
  await expect(slider).toHaveAttribute("aria-valuetext", "50%");
  const save = page.getByRole("button", { name: "保存配置", exact: true });
  await expect(save).toBeEnabled();
  await save.click();
  await expect(save).toBeDisabled();
  for (let step = 1; step <= 15; step++) {
    await expect(slider).toBeEnabled();
    await slider.press("ArrowRight");
    await expect(slider).toHaveValue(((15 + step) / 100).toString());
  }
  await expect(slider).toHaveAttribute("aria-valuetext", "100%");
  await slider.press("ArrowRight");
  await expect(slider).toHaveValue("0.3");
  await expect(save).toBeEnabled();
  await save.click();
  await expect(save).toBeDisabled();
  await page.getByRole("tab", { name: "场景", exact: true }).click();
  await expect(page.locator(".pet-scene-card").first()).toContainText("100%");
});
