import { test, expect, type Page } from "@playwright/test";

const URL = "/iframe.html?id=app-onboarding-runtime--strict-native-transport&viewMode=story";
const tour = (page: Page) => page.locator(".astro-interface-tour");
test.setTimeout(60000);

async function boot(page: Page, options: { resolved?: boolean; readError?: boolean; saveError?: boolean; collapsed?: boolean; locale?: string } = {}) {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.route("**/*", route => {
    const hostname = new globalThis.URL(route.request().url()).hostname;
    return ["localhost", "127.0.0.1"].includes(hostname) ? route.continue() : route.abort();
  });
  await page.addInitScript((options) => {
    const w = window as any;
    w.isTauri = true;
    localStorage.setItem("astro-locale", options.locale ?? "zh");
    if (options.collapsed) localStorage.setItem("astro.sidebarPinned", "0");
    const provider = { id: "qa", kind: "openai", display_name: "QA", model: "qa-small",
      enabled: true, has_api_key: true, key_source: "keyring", supports_responses_api: true };
    let serial = 0;
    const callbacks = new Map();
    w.__tourCalls = [];
    w.__saveError = options.saveError;
    w.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
      transformCallback: (fn: unknown) => { callbacks.set(++serial, fn); return serial; },
      unregisterCallback: (id: number) => callbacks.delete(id),
      convertFileSrc: (path: string) => path,
      invoke: async (cmd: string, args: any = {}) => {
        w.__tourCalls.push({ cmd, args });
        switch (cmd) {
          case "get_onboarding_state": return { version: 1, completed: true, should_show: false, step: "complete" };
          case "get_interface_tour_state":
            if (options.readError) throw Error("unavailable");
            return { resolved_version: Number(localStorage.getItem("qa.tour.version") ?? (options.resolved ? 1 : 0)) };
          case "resolve_interface_tour":
            if (w.__saveError) throw Error("disk full");
            localStorage.setItem("qa.tour.version", "1");
            localStorage.setItem("qa.tour.outcome", args.outcome);
            return null;
          case "get_config": return { agents: [{ id: "default", name: "Astro" }], active_agent_id: "default",
            workspace_dir: "/tmp/qa-default", memory_dir: "/tmp/qa-home", grpc_address: "", default_workspace_dir: "/tmp/qa-default" };
          case "get_providers_state": return { providers: [provider], provider_templates: [], active_provider_id: "qa" };
          case "list_projects": return [{ id: "default", name: "主空间", roots: ["/tmp/qa-default"], position: 0 }];
          case "get_permission_settings": return { preset: "ask_for_approval", sandboxHealth: { status: "available", backend: "qa" } };
          case "get_app_icon": return { current: "blue", options: [] };
          case "get_cached_provider_models": return { models: [], latency_ms: 0, source: "qa" };
          case "get_mcp_servers": case "get_agent_tools": return [];
          default:
            if (cmd.startsWith("get_")) throw Error("QA read unavailable: " + cmd);
            if (cmd.startsWith("list_")) return [];
            if (cmd === "plugin:event|listen") return ++serial;
            return null;
        }
      },
    };
  }, options);
  await page.goto(URL);
  await expect(page.locator(".app-shell")).toBeVisible({ timeout: 30000 });
}

async function expectOutcome(page: Page, outcome: string) {
  await expect(tour(page)).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => localStorage.getItem("qa.tour.outcome"))).toBe(outcome);
  await expect(page.locator(".app-shell")).not.toHaveAttribute("inert", "");
  const mutations = await page.evaluate(() => (window as any).__tourCalls.filter(({ cmd }: any) =>
    /^(start_chat|reset_onboarding_state|save_provider|set_active_provider_model)$/.test(cmd)));
  expect(mutations).toEqual([]);
}

test("five real targets, completion, reload suppression and sidebar replay", async ({ page }, testInfo) => {
  await boot(page);
  await expect(tour(page)).toContainText("花半分钟");
  await expect(tour(page).locator(".driver-popover-progress-text")).toBeHidden();
  await page.screenshot({ path: testInfo.outputPath("welcome.png") });
  await tour(page).getByRole("button", { name: "带我了解", exact: true }).click();
  for (const [index, id] of ["composer", "model", "sidebar", "plugins", "settings"].entries()) {
    await expect(page.locator(`[data-tour="${id}"].driver-active-element`)).toBeVisible();
    await expect(tour(page).locator(".driver-popover-progress-text")).toHaveText(`${index + 1} / 5`);
    if (index === 0 || index === 4) await page.screenshot({ path: testInfo.outputPath(`${id}.png`) });
    await tour(page).getByRole("button", { name: index === 4 ? "开始使用" : "下一步", exact: true }).click();
  }
  await expectOutcome(page, "completed");
  await page.reload();
  await expect(page.locator(".app-shell")).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__tourCalls.some((c: any) => c.cmd === "get_interface_tour_state"))).toBe(true);
  await expect(tour(page)).toHaveCount(0);
  await page.getByRole("button", { name: "界面导览", exact: true }).click();
  await expect(tour(page)).toContainText("花半分钟");
});

test("direct start persists skip and preserves collapsed sidebar preference", async ({ page }) => {
  await boot(page, { collapsed: true });
  await expect(tour(page)).toBeVisible();
  await expect(page.locator(".sidebar")).toHaveClass(/is-open/);
  await tour(page).getByRole("button", { name: "直接开始", exact: true }).click();
  await expectOutcome(page, "skipped");
  await expect(page.locator(".sidebar")).toHaveClass(/is-collapsed/);
  expect(await page.evaluate(() => localStorage.getItem("astro.sidebarPinned"))).toBe("0");
});

test("keyboard focus stays in tour, Escape skips, app shortcuts do not fire", async ({ page }) => {
  await boot(page);
  await expect(tour(page).getByRole("button", { name: "带我了解", exact: true })).toBeFocused();
  for (let i = 0; i < 5; i++) {
    await page.keyboard.press("Tab");
    expect(await tour(page).evaluate(node => node.contains(document.activeElement))).toBe(true);
  }
  await page.keyboard.press("Meta+n");
  await page.keyboard.press("ArrowRight");
  await expect(tour(page)).toContainText("从一个任务开始");
  await page.keyboard.press("ArrowLeft");
  await expect(tour(page)).toContainText("花半分钟");
  await page.keyboard.press("Escape");
  await expectOutcome(page, "skipped");
});

test("failed saves are visible and retryable without trapping the user", async ({ page }) => {
  await boot(page, { saveError: true });
  await tour(page).getByRole("button", { name: "直接开始", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("导览状态未保存");
  expect(await page.locator(".app-shell").evaluate(node => (node as HTMLElement).inert)).toBe(false);
  await page.evaluate(() => { (window as any).__saveError = false; });
  await page.getByRole("button", { name: "重试保存", exact: true }).click();
  await expectOutcome(page, "skipped");
  await expect(page.locator(".interface-tour-notice")).toHaveCount(0);
});

test("failed progress read leaves app usable and manual replay works", async ({ page }) => {
  await boot(page, { readError: true });
  await page.getByRole("button", { name: "界面导览", exact: true }).click();
  await expect(tour(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expectOutcome(page, "skipped");
  await expect(page.getByRole("button", { name: "界面导览", exact: true })).toBeFocused();
});

test("settings about replays without resetting initialization", async ({ page }) => {
  await boot(page, { resolved: true });
  await page.locator('[data-tour="settings"]').click();
  await page.getByRole("button", { name: "关于 Astro", exact: true }).click();
  await page.getByRole("button", { name: "重新查看", exact: true }).click();
  await expect(tour(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expectOutcome(page, "skipped");
});

test("compact dark English tour stays within viewport", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 820, height: 650 });
  await boot(page, { locale: "en" });
  await page.evaluate(() => { document.documentElement.dataset.theme = "dark"; });
  await expect(tour(page)).toContainText("Meet Astro");
  await tour(page).getByRole("button", { name: "Show me around", exact: true }).click();
  for (let i = 0; i < 5; i++) {
    if (i === 0) await page.screenshot({ path: testInfo.outputPath("compact-dark.png") });
    const arrowMatches = await tour(page).evaluate(node => {
      const arrow = node.querySelector<HTMLElement>(".driver-popover-arrow")!;
      const color = getComputedStyle(node).backgroundColor;
      const style = getComputedStyle(arrow);
      return [style.borderTopColor, style.borderBottomColor, style.borderLeftColor, style.borderRightColor]
        .every(border => border === "rgba(0, 0, 0, 0)" || border === color);
    });
    expect(arrowMatches).toBe(true);
    const bounds = await tour(page).boundingBox();
    expect(bounds).toBeTruthy();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.y).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(820);
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(650);
    await tour(page).getByRole("button", { name: i === 4 ? "Get started" : "Next", exact: true }).click();
  }
  await expectOutcome(page, "completed");
});

test("closing the page mid-tour never records a user skip", async ({ page }) => {
  await boot(page);
  await tour(page).getByRole("button", { name: "带我了解", exact: true }).click();
  expect(await page.evaluate(() => (window as any).__tourCalls.filter((c: any) => c.cmd === "resolve_interface_tour"))).toEqual([]);
  await page.reload();
  await expect(tour(page)).toContainText("花半分钟");
  expect(await page.evaluate(() => localStorage.getItem("qa.tour.version"))).toBeNull();
  await page.keyboard.press("Escape");
  await expectOutcome(page, "skipped");
});
