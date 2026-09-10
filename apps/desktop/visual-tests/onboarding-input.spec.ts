import { test, expect } from "@playwright/test";

for (const reducedMotion of ["reduce", "no-preference"] as const) {
test(`service address pointer focus and selection are not filtered or delayed · ${reducedMotion}`, async ({ page }, testInfo) => {
  await page.addInitScript(() => {
    localStorage.setItem("astro-locale", "zh");
    localStorage.setItem("astro-theme-mode", "dark");
  });
  await page.setViewportSize({ width: 1100, height: 900 });
  await page.emulateMedia({ reducedMotion });
  await page.goto("/iframe.html?id=app-first-run-onboarding--provider&viewMode=story");
  const address = page.getByLabel("服务地址", { exact: true });
  await expect(address).toBeVisible();
  await expect(page.locator(".onboarding-stage--provider")).toHaveCSS("transform", "none");
  await address.fill("https://YOUR_RESOURCE.services.ai.azure.com/openai/v1");
  // Updating controlled input state must not recreate the finished 3D layer.
  await expect(page.locator(".onboarding-stage--provider")).toHaveCSS("transform", "none");
  expect(await address.evaluate(input => {
    let element: Element | null = input.parentElement;
    while (element) {
      const style = getComputedStyle(element);
      if (style.backdropFilter && style.backdropFilter !== "none") return false;
      element = element.parentElement;
    }
    return true;
  })).toBe(true);
  expect(await address.evaluate(input => getComputedStyle(input).transitionDuration)).toBe("0s");
  await page.evaluate(() => document.documentElement.classList.add("theme-transitioning"));
  expect(await address.evaluate(input => getComputedStyle(input).transitionDuration)).toBe("0s");
  expect(await page.getByLabel("API Key", { exact: true }).evaluate(input => getComputedStyle(input).transitionDuration)).toBe("0s");
  await page.evaluate(() => document.documentElement.classList.remove("theme-transitioning"));
  await page.evaluate(() => {
    const input = document.querySelector<HTMLInputElement>('input[inputmode="url"]')!;
    (window as any).__focusPaintTimes = [];
    input.addEventListener("pointerdown", () => {
      const start = performance.now();
      requestAnimationFrame(() => requestAnimationFrame(() => {
        (window as any).__focusPaintTimes.push(performance.now() - start);
      }));
    });
  });
  const measurements: Record<string, unknown> = {};
  const variants = process.env.ASTRO_FOCUS_BENCH === "1" ? ["current", "legacy-blur", "restored"] : ["current"];
  for (const variant of variants) {
    if (variant === "legacy-blur") await page.addStyleTag({ content: ".onboarding-card { backdrop-filter: blur(32px) saturate(155%) !important; -webkit-backdrop-filter: blur(32px) saturate(155%) !important; } /* focus-bench */" });
    if (variant === "restored") await page.evaluate(() => {
      document.querySelectorAll("style").forEach(el => { if (el.textContent?.includes("focus-bench")) el.remove(); });
    });
    await page.evaluate(() => { (window as any).__focusPaintTimes = []; });
    for (let index = 0; index < 12; index++) {
      await page.locator(".onboarding-heading h1").click();
      await address.click({ position: { x: 90 + index * 15, y: 20 } });
      await expect(address).toBeFocused();
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    }
    measurements[variant] = await page.evaluate(() => {
      const times = (window as any).__focusPaintTimes as number[];
      const input = document.querySelector<HTMLInputElement>('input[inputmode="url"]')!;
      return { count: times.length, maxMs: Math.max(...times), meanMs: times.reduce((a,b) => a+b, 0)/times.length,
        caret: input.selectionStart, dragRegion: Boolean(input.closest("[data-tauri-drag-region]")) };
    });
  }
  const box = await address.boundingBox();
  if (!box) throw Error("Address field has no bounds");
  await page.mouse.move(box.x + 25, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + 235, box.y + box.height / 2, { steps: 8 });
  await page.mouse.up();
  const selection = await address.evaluate((input: HTMLInputElement) => [input.selectionStart!, input.selectionEnd!]);
  expect(selection[1] - selection[0]).toBeGreaterThan(6);
  await expect(address).toBeFocused();
  await expect(address).toHaveValue("https://YOUR_RESOURCE.services.ai.azure.com/openai/v1");
  expect(await address.evaluate(input => {
    for (let element = input.parentElement; element; element = element.parentElement) {
      const style = getComputedStyle(element);
      if (style.transform !== "none" || style.perspective !== "none" || style.transformStyle === "preserve-3d" || style.willChange.split(",").map(value => value.trim()).includes("transform")) return false;
    }
    return true;
  })).toBe(true);
  console.log(JSON.stringify({ engine: testInfo.project.use.browserName, measurements }));
});
}
