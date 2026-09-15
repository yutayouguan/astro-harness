import { test, expect, type Page } from "@playwright/test";

const URL = "/iframe.html?id=app-onboarding-runtime--strict-native-transport&viewMode=story";
const tour = (page: Page) => page.locator(".astro-interface-tour");
const TOUR_TARGETS = ["composer", "model", "toolbar", "sidebar", "workspace", "plugins", "appearance", "settings"];
test.setTimeout(60000);

async function boot(page: Page, options: { resolved?: boolean; readError?: boolean; saveError?: boolean; collapsed?: boolean; locale?: string; petVisible?: boolean; labels?: boolean; petReadError?: boolean } = {}) {
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
    if (options.labels) localStorage.setItem("astro.sidebarLabels", "1");
    const provider = { id: "qa", kind: "openai", display_name: "QA", model: "qa-small",
      enabled: true, has_api_key: true, key_source: "keyring", supports_responses_api: true };
    let serial = 0;
    const callbacks = new Map();
    const listeners = new Map<number, { event: string; handler: number }>();
    let petRevision = 1;
    w.__petVisible = options.petVisible ?? false;
    w.__petReadError = options.petReadError ?? false;
    w.__petFailMutation = false;
    w.__petMutationDelay = 0;
    const petState = () => ({ revision: petRevision, enabled: true, petPath: "/tmp/qa-pet.png" });
    w.__emitPetChanged = () => {
      petRevision++;
      for (const [id, listener] of listeners) if (listener.event === "desktop-pet-changed") {
        callbacks.get(listener.handler)?.({ id, event: listener.event, payload: petState() });
      }
    };
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
          case "plugin:event|listen": {
            const id = ++serial;
            listeners.set(id, { event: args.event, handler: args.handler });
            return id;
          }
          case "plugin:event|unlisten": listeners.delete(args.eventId); return null;
          case "get_desktop_pet_state": return petState();
          case "get_desktop_pet_visible":
            if (w.__petReadError) throw Error("visibility unavailable");
            return w.__petVisible;
          case "resume_desktop_pet":
          case "set_desktop_pet_enabled":
            if (w.__petFailMutation) throw Error("pet window failed");
            if (w.__petMutationDelay) await new Promise(resolve => setTimeout(resolve, w.__petMutationDelay));
            w.__petVisible = cmd === "resume_desktop_pet";
            w.__emitPetChanged();
            return petState();
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

test("eight real targets, completion, reload suppression and sidebar replay", async ({ page }, testInfo) => {
  await boot(page);
  await expect(tour(page)).toContainText("花半分钟");
  await expect(tour(page).locator(".driver-popover-progress-text")).toBeHidden();
  await page.screenshot({ path: testInfo.outputPath("welcome.png") });
  await tour(page).getByRole("button", { name: "带我了解", exact: true }).click();
  for (const [index, id] of TOUR_TARGETS.entries()) {
    await expect(page.locator(`[data-tour="${id}"].driver-active-element`)).toBeVisible();
    await expect(tour(page).locator(".driver-popover-progress-text")).toHaveText(`${index + 1} / ${TOUR_TARGETS.length}`);
    if (["workspace", "toolbar", "appearance"].includes(id)) await page.screenshot({ path: testInfo.outputPath(`${id}.png`) });
    await tour(page).getByRole("button", { name: index === TOUR_TARGETS.length - 1 ? "开始使用" : "下一步", exact: true }).click();
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
  for (let i = 0; i < TOUR_TARGETS.length; i++) {
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
    await tour(page).getByRole("button", { name: i === TOUR_TARGETS.length - 1 ? "Get started" : "Next", exact: true }).click();
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

test("footer places icon-only tour and pet after preferences", async ({ page }, testInfo) => {
  await boot(page, { resolved: true, labels: true });
  const footer = page.locator(".sidebar-footer-actions");
  const buttons = footer.getByRole("button");
  await expect(buttons).toHaveCount(3);
  await expect(buttons.nth(0)).toHaveAttribute("data-tour", "settings");
  await expect(buttons.nth(1)).toHaveAttribute("data-sidebar-action", "tour");
  await expect(buttons.nth(2)).toHaveAttribute("data-sidebar-action", "pet");
  await expect(buttons.nth(1)).toHaveText("");
  await expect(buttons.nth(2)).toHaveText("");
  const bounds = await Promise.all([0, 1, 2].map(i => buttons.nth(i).boundingBox()));
  expect(Math.abs(bounds[0]!.y - bounds[1]!.y)).toBeLessThan(2);
  expect(bounds[1]!.x).toBeGreaterThan(bounds[0]!.x);
  expect(bounds[2]!.x).toBeGreaterThan(bounds[1]!.x);
  await footer.screenshot({ path: testInfo.outputPath("footer-actions.png") });
  await page.getByRole("button", { name: "界面导览", exact: true }).click();
  await expect(tour(page)).toBeVisible();
});

test("pet icon toggles actual visibility and follows external changes", async ({ page }) => {
  // Persisted enabled=true but the actual native window is hidden (e.g. presentation mode).
  await boot(page, { resolved: true });
  const button = page.locator('[data-sidebar-action="pet"]');
  await expect(button).toHaveAccessibleName("显示桌宠");
  await expect(button).toHaveAttribute("aria-pressed", "false");
  await button.click();
  await expect(button).toHaveAccessibleName("隐藏桌宠");
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await button.click();
  await expect(button).toHaveAccessibleName("显示桌宠");
  expect(await page.evaluate(() => (window as any).__tourCalls.filter((c: any) =>
    ["resume_desktop_pet", "set_desktop_pet_enabled"].includes(c.cmd)))).toEqual([
      { cmd: "resume_desktop_pet", args: {} },
      { cmd: "set_desktop_pet_enabled", args: { enabled: false } },
    ]);
  await page.evaluate(() => { (window as any).__petVisible = true; (window as any).__emitPetChanged(); });
  await expect(button).toHaveAccessibleName("隐藏桌宠");
});

test("pet toggle blocks duplicate clicks and reports native failure", async ({ page }) => {
  await boot(page, { resolved: true, petVisible: true });
  const button = page.locator('[data-sidebar-action="pet"]');
  await expect(button).toHaveAccessibleName("隐藏桌宠");
  await page.evaluate(() => { (window as any).__petMutationDelay = 300; });
  await button.evaluate(node => { (node as HTMLButtonElement).click(); (node as HTMLButtonElement).click(); });
  await expect(button).toBeDisabled();
  await expect(button).toHaveAccessibleName("显示桌宠");
  await expect(button).toBeEnabled();
  expect(await page.evaluate(() => (window as any).__tourCalls.filter((c: any) => c.cmd === "set_desktop_pet_enabled").length)).toBe(1);
  await page.evaluate(() => { (window as any).__petFailMutation = true; });
  await button.click();
  await expect(page.getByText("桌宠显隐切换未完成，请重试或前往偏好设置检查桌宠。", { exact: true })).toBeVisible();
  await expect(button).toHaveAttribute("aria-pressed", "false");
  await expect(button).toBeEnabled();
});

test("unknown pet visibility retries without guessing or mutating", async ({ page }) => {
  await boot(page, { resolved: true, petReadError: true });
  const button = page.locator('[data-sidebar-action="pet"]');
  await expect(button).toHaveAccessibleName("重新读取桌宠状态");
  await expect(button).toBeEnabled();
  await page.evaluate(() => { (window as any).__petReadError = false; });
  await button.click();
  await expect(button).toHaveAccessibleName("显示桌宠");
  expect(await page.evaluate(() => (window as any).__tourCalls.filter((c: any) =>
    ["resume_desktop_pet", "set_desktop_pet_enabled"].includes(c.cmd)))).toEqual([]);
});

test("non-dynamic mode without wallpaper omits the appearance step", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("astro-shell-color-prefs.v2", JSON.stringify({ style: "unified" }));
  });
  await boot(page);
  await expect(page.locator('[data-tour="appearance"]')).toHaveCount(0);
  await tour(page).getByRole("button", { name: "带我了解", exact: true }).click();
  const targets = TOUR_TARGETS.filter(id => id !== "appearance");
  for (const [index, id] of targets.entries()) {
    await expect(page.locator(`[data-tour="${id}"].driver-active-element`)).toBeVisible();
    await expect(tour(page).locator(".driver-popover-progress-text")).toHaveText(`${index + 1} / ${targets.length}`);
    await tour(page).getByRole("button", { name: index === targets.length - 1 ? "开始使用" : "下一步", exact: true }).click();
  }
  await expectOutcome(page, "completed");
});
