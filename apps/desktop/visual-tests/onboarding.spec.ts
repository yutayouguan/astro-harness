import { test, expect, type Page } from "@playwright/test";

test.setTimeout(60000);
const RUNTIME_URL = "/iframe.html?id=app-onboarding-runtime--native-transport&viewMode=story";
type Boot = "fresh" | "existing" | "corrupt";
async function installTransport(page: Page, boot: Boot = "fresh") {
  await page.emulateMedia({ reducedMotion: "reduce" });
  // No external/provider requests are permitted in this suite.
  await page.route("**/*", async route => {
    const url = new URL(route.request().url());
    if (!["127.0.0.1", "localhost"].includes(url.hostname)) {
      await route.abort("blockedbyclient"); return;
    }
    await route.continue();
  });
  await page.route("**/__onboarding_mock/responses", route => route.fulfill({
    json: { ok: true, model: "qa-small", latency_ms: 12, message: "ok" },
  }));
  await page.route("**/__onboarding_mock/models", route => route.fulfill({
    json: { models: [{ id: "qa-small" }, { id: "qa-alt" }, { id: "text-embedding-3-small" }], latency_ms: 1, source: "qa" },
  }));
  await page.addInitScript(({ boot }) => {
    const w = window as any;
    const initialDraft = { agent_name: "Astro", provider_id: "", model: "", endpoint: "",
      workspace_path: "", permission_preset: "ask_for_approval" };
    const fresh = { version: 1, step: "intro", completed: false, should_show: true,
      inferred_existing_install: false, updated_at: null, draft: initialDraft };
    if (!localStorage.getItem("qa.initialized")) {
      localStorage.setItem("qa.initialized", "true");
      localStorage.setItem("astro-locale", "zh");
      localStorage.setItem("astro-theme-mode", "light");
      localStorage.setItem("qa.state", JSON.stringify(boot === "existing"
        ? { ...fresh, completed: true, should_show: false, inferred_existing_install: true } : fresh));
      if (boot === "corrupt") localStorage.setItem("qa.corrupt", "true");
    }
    const provider = { id: "qa", kind: "openai", display_name: "Local QA Provider",
      endpoint: "http://127.0.0.1/mock/v1", model: "qa-small", enabled: true,
      has_api_key: false, key_source: "none", env_key_name: null, backend_id: "openai",
      official_key_url: "https://platform.openai.com/",
      supports_responses_api: true };
    const initialProviders = { providers: [], provider_templates: [provider], active_provider_id: null, active_image_provider_id: null };
    if (!localStorage.getItem("qa.providers")) localStorage.setItem("qa.providers", JSON.stringify(initialProviders));
    const readState = () => JSON.parse(localStorage.getItem("qa.state")!);
    const readProviders = () => JSON.parse(localStorage.getItem("qa.providers")!);
    if (!localStorage.getItem("qa.preferences")) localStorage.setItem("qa.preferences", JSON.stringify({
      notifications: false, autostart: false,
      pet: { enabled: false, hasAsset: false, previewPath: null, spriteVersionNumber: null, displayName: null },
    }));
    const readPreferences = () => JSON.parse(localStorage.getItem("qa.preferences")!);
    let projects = [{ id: "default", name: "主空间", roots: ["/tmp/qa-default"], position: 0, createdAt: "", updatedAt: "" }];
    let agentName = "Astro";
    let receipt: string | null = null;
    const snapshot = () => JSON.stringify(readProviders().providers[0]);
    w.__onboardingCalls = [];
    w.__failProgress = false;
    const callbacks = new Map<number, any>();
    let serial = 0;
    w.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
      transformCallback: (fn: any) => { callbacks.set(++serial, fn); return serial; },
      unregisterCallback: (id: number) => callbacks.delete(id),
      runCallback: (id: number, data: any) => callbacks.get(id)?.(data),
      convertFileSrc: (path: string) => path,
      invoke: async (cmd: string, args: any = {}) => {
        w.__onboardingCalls.push({ cmd, args: { ...args, apiKey: args.apiKey ? "[test value]" : undefined } });
        switch (cmd) {
          case "get_onboarding_state":
            if (localStorage.getItem("qa.corrupt")) throw Error("corrupt state");
            return readState();
          case "save_onboarding_progress": {
            if (w.__failProgress) throw Error("disk write failed");
            if (readState().completed) return readState();
            const state = { ...readState(), step: args.step, draft: args.draft };
            localStorage.setItem("qa.state", JSON.stringify(state)); return state;
          }
          case "complete_onboarding": {
            if (args.verificationToken !== "qa-proof" || receipt !== snapshot() || w.__invalidateProof) throw Error("ONBOARDING_VERIFICATION_REQUIRED");
            const state = { ...readState(), step: "complete", completed: true, should_show: false };
            if (typeof state.draft.pet_enabled === "boolean") {
              const prefs = readPreferences();
              prefs.pet.enabled = state.draft.pet_enabled;
              localStorage.setItem("qa.preferences", JSON.stringify(prefs));
            }
            localStorage.setItem("qa.state", JSON.stringify(state)); return state;
          }
          case "get_providers_state":
            if (w.__failProviders || localStorage.getItem("qa.failProviders")) throw Error("network unavailable");
            return readProviders();
          case "list_providers": return readProviders().providers;
          case "save_provider": {
            const state = readProviders();
            const existing = [...state.providers, ...state.provider_templates].find(p => p.id === args.provider.id);
            state.providers = [{ ...existing, ...args.provider }];
            state.provider_templates = state.provider_templates.filter(p => p.id !== args.provider.id);
            localStorage.setItem("qa.providers", JSON.stringify(state)); return state;
          }
          case "set_provider_api_key": {
            const state = readProviders();
            state.providers[0].has_api_key = true; state.providers[0].key_source = "keyring";
            localStorage.setItem("qa.providers", JSON.stringify(state)); return state;
          }
          case "verify_onboarding_provider": {
            const before = snapshot();
            const result = await fetch("/__onboarding_mock/responses", { method: "POST", body: JSON.stringify({ model: args.model }) }).then(r => r.json());
            if (result.ok && before === snapshot()) receipt = before;
            return { ...result, verification_token: result.ok && !w.__omitProof ? "qa-proof" : null };
          }
          case "set_active_provider_model": {
            const state = readProviders(); state.active_provider_id = args.id;
            localStorage.setItem("qa.providers", JSON.stringify(state)); return state;
          }
          case "add_provider": {
            const state = readProviders();
            state.providers.push({ ...provider, id: "qa-added", display_name: "OpenAI", config_source: "user" });
            localStorage.setItem("qa.providers", JSON.stringify(state)); return state;
          }
          case "delete_provider": {
            const state = readProviders(); state.providers = state.providers.filter(p => p.id !== args.id);
            state.active_provider_id = null;
            localStorage.setItem("qa.providers", JSON.stringify(state)); return state;
          }
          case "get_config":
            if (localStorage.getItem("qa.failWorkspaceConfig")) throw Error("configuration unavailable");
            return { agents: [{ id: "default", name: agentName }], active_agent_id: "default",
              workspace_dir: "/tmp/qa-default", memory_dir: "/tmp/qa-home", grpc_address: "",
              default_workspace_dir: "/tmp/qa-default",
              default_workspace_display_path: "/tmp/qa-default" };
          case "get_onboarding_pet": return readPreferences().pet;
          case "get_task_notifications_enabled": return readPreferences().notifications;
          case "get_desktop_autostart": return readPreferences().autostart;
          case "set_task_notifications_enabled":
          case "set_desktop_autostart": {
            if (w.__denyPreference === cmd) throw Error("preference denied");
            const prefs = readPreferences();
            prefs[cmd === "set_desktop_autostart" ? "autostart" : "notifications"] = args.enabled;
            localStorage.setItem("qa.preferences", JSON.stringify(prefs));
            return args.enabled;
          }
          case "check_onboarding_workspace":
            return { path: args.path, status: w.__workspaceStatus ?? "ready" };
          case "set_default_agent_name": agentName = args.name; return { id: "default", name: agentName };
          case "get_permission_settings":
          case "set_permission_preset": return { preset: args.preset ?? "ask_for_approval", sandboxHealth: { status: "available", backend: "qa" } };
          case "plugin:dialog|open": return "/tmp/qa-workspace";
          case "list_projects": return projects;
          case "create_project": {
            const project = { id: "qa-project", name: args.name, roots: args.roots, position: 1, createdAt: "", updatedAt: "" };
            projects = [...projects, project]; return project;
          }
          case "get_app_icon": return { current: "blue", options: [] };
          case "get_mcp_servers": return [];
          case "get_cached_provider_models": return { models: [], latency_ms: 0, source: "qa" };
          case "list_provider_models":
            return await fetch("/__onboarding_mock/models").then(r => r.json()).then(result => {
              if (result.error) throw Error(result.error);
              return result;
            });
          case "get_system_wallpaper": throw Error("No system wallpaper in QA");
          case "get_agent_tools": return [];
          case "start_chat": throw Error("TEST FAILURE: no task should auto-send");
          default:
            if (cmd.startsWith("get_")) throw Error("Read unavailable in QA: " + cmd);
            if (cmd.startsWith("list_")) return [];
            if (cmd === "plugin:event|listen") return ++serial;
            return null;
        }
      },
    };
  }, { boot });
}

async function startProvider(page: Page) {
  await page.goto(RUNTIME_URL);
  const skip = page.getByRole("button", { name: "跳过动画", exact: true });
  if (await skip.isVisible()) await skip.click();
  await expect(page.getByRole("heading", { name: "先让这里更像你的工作空间" })).toBeVisible({ timeout: 30000 });
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.getByRole("heading", { name: "为 Astro 接入思考能力" })).toBeVisible();
}
async function chooseModel(page: Page, model = "qa-small") {
  const picker = page.getByRole("button", { name: "默认模型", exact: true });
  if (await picker.getAttribute("aria-expanded") !== "true") await picker.click();
  await page.getByRole("option", { name: model, exact: true }).click();
}
async function loadModels(page: Page) {
  await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
  await expect(page.getByRole("button", { name: "默认模型", exact: true })).toBeEnabled();
  await expect(page.getByRole("combobox", { name: "搜索模型名称或 ID" })).toBeVisible();
  await chooseModel(page);
}
async function verifyProvider(page: Page) {
  await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
  await loadModels(page);
  await page.getByRole("button", { name: "测试", exact: true }).click();
  await expect(page.getByText("连接成功", { exact: true })).toBeVisible();
}
async function finishVerified(page: Page) {
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await page.getByRole("button", { name: "完成设置", exact: true }).click();
  await expect(page.getByRole("heading", { name: "一切准备就绪", exact: true })).toBeVisible();
}

test("optional preferences default off, pet stays pending until completion", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await page.getByRole("button", { name: "返回", exact: true }).click();
  for (const name of ["桌面宠物", "任务通知", "登录时启动"]) {
    await expect(page.getByRole("switch", { name, exact: true })).toBeEnabled();
    await expect(page.getByRole("switch", { name, exact: true })).not.toBeChecked();
  }
  await expect(page.locator(".onboarding-pet-preference svg.lucide-paw-print")).toHaveCount(1);
  await expect(page.locator(".onboarding-pet-preference img, .onboarding-pet-preview")).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => ["set_task_notifications_enabled", "set_desktop_autostart"].includes(x.cmd)))).toEqual([]);
  await page.getByRole("switch", { name: "桌面宠物", exact: true }).click();
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.pet_enabled)).toBe(true);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.preferences")!).pet.enabled)).toBe(false);
  await page.getByRole("switch", { name: "任务通知", exact: true }).click();
  await expect(page.getByRole("switch", { name: "任务通知", exact: true })).toBeChecked();
  await page.getByRole("switch", { name: "登录时启动", exact: true }).click();
  await expect(page.getByRole("switch", { name: "登录时启动", exact: true })).toBeChecked();
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await verifyProvider(page);
  await finishVerified(page);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.preferences")!).pet.enabled)).toBe(true);
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => x.cmd.includes("generate_desktop_pet")))).toEqual([]);
});

test("missing existing pet is not silently replaced with the builtin companion", async ({ page }) => {
  await installTransport(page);
  await page.addInitScript(() => localStorage.setItem("qa.preferences", JSON.stringify({
    notifications: false, autostart: false,
    pet: { enabled: false, hasAsset: false, previewPath: "/missing/user-pet.png", spriteVersionNumber: null, displayName: "Existing pet" },
  })));
  await startProvider(page);
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await expect(page.getByRole("switch", { name: "桌面宠物", exact: true })).toBeDisabled();
  await expect(page.getByText("原有资源不可用，请在桌宠设置中重新选择。", { exact: true })).toBeVisible();
  await expect(page.getByAltText("桌面小伙伴预览")).toHaveCount(0);
});

test("rerun preserves current preferences when unchanged", async ({ page }) => {
  await installTransport(page);
  await page.addInitScript(() => localStorage.setItem("qa.preferences", JSON.stringify({
    notifications: true, autostart: true,
    pet: { enabled: true, hasAsset: true, previewPath: null, spriteVersionNumber: null, displayName: "Existing pet" },
  })));
  await startProvider(page);
  await page.getByRole("button", { name: "返回", exact: true }).click();
  for (const name of ["桌面宠物", "任务通知", "登录时启动"]) await expect(page.getByRole("switch", { name, exact: true })).toBeChecked();
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await verifyProvider(page);
  await finishVerified(page);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.preferences")!))).toMatchObject({ notifications: true, autostart: true, pet: { enabled: true } });
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => ["set_task_notifications_enabled", "set_desktop_autostart"].includes(x.cmd)))).toEqual([]);
});

for (const [label, cmd] of [["任务通知", "set_task_notifications_enabled"], ["登录时启动", "set_desktop_autostart"]]) {
  test(`failed optional preference rolls back: ${label}`, async ({ page }) => {
    await installTransport(page);
    await startProvider(page);
    await page.getByRole("button", { name: "返回", exact: true }).click();
    await page.evaluate(cmd => { (window as any).__denyPreference = cmd; }, cmd);
    const control = page.getByRole("switch", { name: label, exact: true });
    await control.click();
    await expect(page.locator(".desktop-preference-error")).toBeVisible();
    await expect(control).not.toBeChecked();
    await expect(control).toBeEnabled();
    await page.evaluate(() => { (window as any).__denyPreference = null; });
    await control.click();
    await expect(control).toBeChecked();
  });
}

test("model connection is mandatory; credentials alone do not unlock setup", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: /稍后连接|先进入 App|跳过设置/ })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "测试", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "保存密钥并获取模型", exact: true })).toBeDisabled();
  await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
  await loadModels(page);
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) =>
    ["complete_onboarding", "start_chat"].includes(x.cmd)))).toEqual([]);
});

test("verified setup prepares an editable App draft without sending", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 720 });
  await installTransport(page);
  await startProvider(page);
  await verifyProvider(page);
  expect(await page.locator(".onboarding-stage--provider").evaluate(el => el.scrollHeight - el.clientHeight)).toBeLessThanOrEqual(2);
  await finishVerified(page);
  await expect(page.getByText("本次连接测试已通过", { exact: true })).toBeVisible();
  expect(await page.locator(".onboarding-stage--complete").evaluate(el => el.scrollHeight - el.clientHeight)).toBeLessThanOrEqual(2);
  await expect(page.locator(".onboarding-task-card")).toHaveCount(6);
  await page.getByRole("button", { name: "看看我的桌面 · 填入输入框", exact: true }).click();
  await expect(page.locator(".app-shell")).toBeVisible({ timeout: 30000 });
  await expect(page.locator(".onboarding-root")).toHaveCount(0);
  await expect(page.locator(".onboarding-brand-motion")).toHaveCount(0);
  await expect(page.locator(".composer-input")).toHaveValue(/请看看我桌面的文件.*至少 5 个/);
  await expect(page.locator(".composer-input")).toBeEditable();
  await expect(page.getByRole("button", { name: "发送", exact: true })).toBeEnabled();
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => x.cmd === "start_chat"))).toEqual([]);
});

test("restart restores draft but requires model verification again", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await page.getByLabel("Agent 名称（可选）").fill("Nova");
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await verifyProvider(page);
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await page.getByRole("button", { name: /使用默认工作空间/ }).click();
  await expect(page.locator(".onboarding-folder-picker small")).toHaveText("/tmp/qa-workspace");
  await page.getByText("自动处理常规操作", { exact: true }).click();
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.permission_preset)).toBe("approve_for_me");
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.workspace_path)).toBe("/tmp/qa-workspace");
  const saved = await page.evaluate(() => localStorage.getItem("qa.state")!);
  expect(saved).not.toContain("qa-placeholder-key");
  expect(saved).not.toContain("qa-proof");
  await page.reload();
  await expect(page.getByRole("heading", { name: "为 Astro 接入思考能力" })).toBeVisible({ timeout: 30000 });
  await expect(page.getByLabel("API Key", { exact: true })).toHaveValue("");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  await loadModels(page);
  await page.getByRole("button", { name: "测试", exact: true }).click();
  await expect(page.getByText("连接成功", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.locator(".onboarding-folder-picker small")).toHaveText("/tmp/qa-workspace");
  await expect(page.getByRole("radio", { name: /自动处理常规操作/ })).toBeChecked();
  await page.getByRole("button", { name: "完成设置", exact: true }).click();
  await expect(page.getByText("Nova", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "空白开始", exact: true }).click();
  await expect(page.locator(".app-shell")).toBeVisible({ timeout: 30000 });
  expect(await page.evaluate(() => localStorage.getItem("astro.activeProjectId"))).toBe("qa-project");
});

for (const [message, title] of [
  ["401 invalid api key echo=qa-canary-secret", "密钥或访问权限有问题"],
  ["429 insufficient_quota qa-canary-secret", "账户余额或额度不足"],
  ["404 model_not_found qa-canary-secret", "模型或部署不存在"],
  ["request timed out qa-canary-secret", "连接测试超时"],
  ["fetch failed network qa-canary-secret", "暂时无法连接服务"],
] as const) {
  test(`connection failure blocks continuation: ${title}`, async ({ page }) => {
    await installTransport(page);
    await page.route("**/__onboarding_mock/responses", route => route.fulfill({ json: { ok: false, message } }));
    await startProvider(page);
    await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
  await loadModels(page);
    await page.getByRole("button", { name: "测试", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText(title);
    await expect(page.locator("body")).not.toContainText("qa-canary-secret");
    await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
    await expect(page.getByRole("button", { name: /稍后连接|先进入 App/ })).toHaveCount(0);
    await page.getByRole("button", { name: "修改配置", exact: true }).click();
    if (title === "密钥或访问权限有问题") await expect(page.getByLabel("API Key", { exact: true })).toBeFocused();
    if (title === "模型或部署不存在") await expect(page.getByLabel("默认模型", { exact: true })).toBeFocused();
    if (title === "连接测试超时" || title === "暂时无法连接服务") await expect(page.getByLabel("服务地址", { exact: true })).toBeFocused();
    await page.unroute("**/__onboarding_mock/responses");
    await page.route("**/__onboarding_mock/responses", route => route.fulfill({ json: { ok: true, model: "qa-small", latency_ms: 1, message: "ok" } }));
    await page.getByRole("button", { name: "重试连接", exact: true }).click();
    await expect(page.getByText("连接成功", { exact: true })).toBeVisible();
  });
}

for (const variant of [
  { theme: "light", width: 1100, contrast: "no-preference" },
  { theme: "dark", width: 390, contrast: "no-preference" },
  { theme: "light", width: 390, contrast: "more" },
] as const) {
  test(`connection issue uses shared actions · ${variant.theme} · ${variant.width} · ${variant.contrast}`, async ({ page }, testInfo) => {
    await installTransport(page);
    await page.addInitScript(theme => localStorage.setItem("astro-theme-mode", theme), variant.theme);
    await page.emulateMedia({ reducedMotion: "reduce", contrast: variant.contrast });
    await page.setViewportSize({ width: variant.width, height: variant.width >= 900 ? 720 : 1000 });
    await page.route("**/__onboarding_mock/responses", route => route.fulfill({
      json: { ok: false, message: "401 invalid api key" },
    }));
    await startProvider(page);
    await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
    await loadModels(page);
    await page.getByRole("button", { name: "测试", exact: true }).click();
    const card = page.getByRole("alert", { name: "密钥或访问权限有问题" });
    await expect(card).toBeVisible();
    if (variant.width >= 900) {
      expect(await page.locator(".onboarding-stage--provider").evaluate(el => el.scrollHeight - el.clientHeight)).toBeLessThanOrEqual(2);
      await expect(page.getByRole("button", { name: "继续", exact: true })).toBeInViewport();
    }
    await expect(card.locator(".onboarding-connection-issue__icon svg")).toHaveCount(1);
    const edit = card.getByRole("button", { name: "修改配置", exact: true });
    const retry = card.getByRole("button", { name: "重试连接", exact: true });
    await expect(edit).toHaveClass(/ui-button--secondary/);
    await expect(retry).toHaveClass(/ui-button--primary/);
    expect(await card.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
    const bounds = await card.boundingBox();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(variant.width);
    await edit.focus();
    await edit.press("Tab");
    await expect(retry).toBeFocused();
    expect(await retry.evaluate(el => getComputedStyle(el).outlineStyle)).not.toBe("none");
    await retry.press("Shift+Tab");
    await expect(edit).toBeFocused();
    expect(await edit.evaluate(el => getComputedStyle(el).outlineStyle)).not.toBe("none");
    await page.screenshot({ path: testInfo.outputPath("connection-issue-page.png"), fullPage: true, animations: "disabled" });
    await card.screenshot({ path: testInfo.outputPath("connection-issue.png"), animations: "disabled" });
    await edit.click();
    await expect(page.getByLabel("API Key", { exact: true })).toBeFocused();
    await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  });
}

test("cancelled test ignores late success and cannot activate a model", async ({ page }, testInfo) => {
  await installTransport(page);
  await page.addInitScript(() => localStorage.setItem("astro-theme-mode", "dark"));
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/__onboarding_mock/responses", async route => {
    await pending; await route.fulfill({ json: { ok: true, model: "qa-small", latency_ms: 1 } });
  });
  await startProvider(page);
  await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
  await loadModels(page);
  const request = page.waitForRequest("**/__onboarding_mock/responses");
  const testButton = page.locator(".onboarding-test-button");
  const testNode = await testButton.elementHandle();
  const before = (await testButton.boundingBox())!;
  await page.getByRole("button", { name: "测试", exact: true }).click();
  await request;
  await expect(testButton).toHaveAccessibleName("取消等待");
  await expect(testButton).toBeEnabled();
  await expect(testButton.locator("svg.lucide-circle-x")).toHaveCount(1);
  await expect(page.locator(".onboarding-load-models")).toBeDisabled();
  await expect(page.getByRole("button", { name: "取消等待", exact: true })).toHaveCount(1);
  await expect(page.locator(".onboarding-cancel-test")).toHaveCount(0);
  await expect(page.locator(".onboarding-test-action").getByRole("status")).toContainText("测试中");
  const during = (await testButton.boundingBox())!;
  for (const key of ["x", "y", "width", "height"] as const) expect(Math.abs(during[key] - before[key])).toBeLessThan(1);
  expect(await testNode!.evaluate(el => el.isConnected)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("test-inline-cancel.png") });
  await testButton.click();
  await expect(testButton).toHaveAccessibleName("测试");
  await expect(testButton).toBeFocused();
  const response = page.waitForResponse("**/__onboarding_mock/responses");
  release(); await response;
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => x.cmd === "set_active_provider_model"))).toEqual([]);
});

test("editing a verified model invalidates continuation", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await verifyProvider(page);
  await chooseModel(page, "qa-alt");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
});

test("backend verification rejection returns to model setup", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await verifyProvider(page);
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await page.evaluate(() => { (window as any).__invalidateProof = true; });
  await page.getByRole("button", { name: "完成设置", exact: true }).click();
  await expect(page.getByRole("heading", { name: "为 Astro 接入思考能力" })).toBeVisible();
  await expect(page.getByRole("alert")).toContainText("需要重新验证模型连接");
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).completed)).toBe(false);
});

test("success without a backend receipt never unlocks setup", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await page.evaluate(() => { (window as any).__omitProof = true; });
  await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
  await loadModels(page);
  await page.getByRole("button", { name: "测试", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("需要重新验证模型连接");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
});

test("existing installation bypasses welcome but missing credentials still block sending", async ({ page }) => {
  await installTransport(page, "existing");
  await page.goto(RUNTIME_URL);
  await expect(page.locator(".app-shell")).toBeVisible({ timeout: 30000 });
  await expect(page.locator(".onboarding-root")).toHaveCount(0);
  await page.locator(".composer-input").fill("这是一条本地草稿");
  await expect(page.locator(".composer-input")).toBeEditable();
  await page.locator(".composer-input").press("Enter");
  await expect(page.getByRole("button", { name: "发送", exact: true })).toBeDisabled();
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => x.cmd === "start_chat"))).toEqual([]);
  await page.getByRole("button", { name: "连接模型", exact: true }).click();
  await expect(page.getByRole("heading", { name: "模型服务", exact: true })).toBeVisible();
});

test("saved credentials are reused without rewriting provider config; only explicit tests generate", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await verifyProvider(page);
  await page.reload();
  await expect(page.getByLabel("API Key", { exact: true })).toHaveValue("");
  await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
  await expect(page.getByRole("combobox", { name: "搜索模型名称或 ID" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "默认模型", exact: true })).toContainText("qa-small");
  let calls = await page.evaluate(() => (window as any).__onboardingCalls);
  expect(calls.filter((x: any) => ["save_provider", "set_provider_api_key", "verify_onboarding_provider"].includes(x.cmd))).toEqual([]);
  await chooseModel(page, "qa-alt");
  await expect(page.getByRole("button", { name: "测试", exact: true })).toBeFocused();
  await page.getByRole("button", { name: "测试", exact: true }).click();
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeFocused();
  await finishVerified(page);
  await page.reload();
  await expect(page.locator(".app-shell")).toBeVisible();
  await expect(page.locator(".onboarding-root")).toHaveCount(0);
});

for (const status of ["missing", "not_directory", "not_writable", "unavailable"]) {
  test(`workspace ${status} blocks completion and supports recheck`, async ({ page }) => {
    await installTransport(page);
    await startProvider(page);
    await verifyProvider(page);
    await page.evaluate(status => { (window as any).__workspaceStatus = status; }, status);
    await page.getByRole("button", { name: "继续", exact: true }).click();
    await expect(page.getByRole("button", { name: "完成设置", exact: true })).toBeDisabled();
    await expect(page.getByRole("button", { name: "重新检查", exact: true })).toBeVisible();
    await page.evaluate(() => { (window as any).__workspaceStatus = "ready"; });
    await page.getByRole("button", { name: "重新检查", exact: true }).click();
    await expect(page.getByText("目录存在，可以读写", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "完成设置", exact: true })).toBeEnabled();
  });
}

test("workspace is rechecked at completion and quota errors offer the official console", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await verifyProvider(page);
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.getByRole("button", { name: "完成设置", exact: true })).toBeEnabled();
  await page.evaluate(() => { (window as any).__workspaceStatus = "not_writable"; });
  await page.getByRole("button", { name: "完成设置", exact: true }).click();
  await expect(page.locator(".onboarding-workspace-check")).toContainText("目录不可写，请更换目录或检查权限");
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => x.cmd === "complete_onboarding"))).toEqual([]);
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "默认模型", exact: true })).toHaveCount(0);
  await page.route("**/__onboarding_mock/responses", route => route.fulfill({ json: { ok: false, message: "insufficient_quota" } }));
  await page.getByRole("button", { name: "重新测试", exact: true }).click();
  await expect(page.getByRole("button", { name: "官方控制台", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "官方控制台", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__onboardingCalls.find((x: any) => x.cmd === "plugin:opener|open_url")?.args.url)).toBe("https://platform.openai.com/");
});

test("revisiting provider keeps the model menu closed until explicitly reloaded", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await verifyProvider(page);
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.getByRole("button", { name: "完成设置", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await expect(page.getByRole("heading", { name: "为 Astro 接入思考能力" })).toBeVisible();
  await expect(page.getByRole("dialog", { name: "默认模型", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "默认模型", exact: true })).toBeVisible();
});

test("state read error provides retry but no bypass", async ({ page }) => {
  await installTransport(page, "corrupt");
  await page.goto(RUNTIME_URL);
  await expect(page.getByRole("heading", { name: "暂时无法读取初始化状态" })).toBeVisible({ timeout: 30000 });
  await expect(page.getByRole("button", { name: /先进入|Enter app/ })).toHaveCount(0);
  await page.evaluate(() => localStorage.removeItem("qa.corrupt"));
  await page.getByRole("button", { name: "重试", exact: true }).click();
  await expect(page.getByRole("button", { name: "跳过动画", exact: true })).toBeVisible();
});

test("workspace uses the backend path and activates that project after confirmation", async ({ page }, testInfo) => {
  await installTransport(page);
  await startProvider(page);
  await verifyProvider(page);
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.locator(".onboarding-folder-picker small")).toHaveText("/tmp/qa-default");
  await expect(page.locator(".onboarding-folder-picker small")).toHaveAttribute("title", "/tmp/qa-default");
  await expect(page.getByText(/请确认工作空间路径/)).toBeVisible();
  await page.getByRole("button", { name: /使用默认工作空间/ }).click();
  await expect(page.locator(".onboarding-folder-picker small")).toHaveText("/tmp/qa-workspace");
  await page.getByRole("button", { name: "使用默认工作空间", exact: true }).click();
  await expect(page.locator(".onboarding-folder-picker small")).toHaveText("/tmp/qa-default");
  await page.evaluate(() => localStorage.setItem("astro.activeProjectId", "stale-project"));
  await page.getByRole("button", { name: "完成设置", exact: true }).click();
  await expect(page.getByRole("heading", { name: "一切准备就绪", exact: true })).toBeVisible();
  await expect(page.locator(".onboarding-health-grid")).toContainText("/tmp/qa-default");
  expect(await page.evaluate(() => localStorage.getItem("astro.activeProjectId"))).toBe("default");
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => x.cmd === "create_project"))).toEqual([]);
});

test("unreadable default workspace cannot be confirmed until retry succeeds", async ({ page }) => {
  await installTransport(page);
  await page.addInitScript(() => localStorage.setItem("qa.failWorkspaceConfig", "true"));
  await startProvider(page);
  await verifyProvider(page);
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.locator(".onboarding-workspace-error")).toContainText("无法确认默认工作空间路径");
  await expect(page.getByRole("button", { name: "完成设置", exact: true })).toBeDisabled();
  await expect(page.locator(".onboarding-folder-picker small")).not.toContainText("~/.astro/workspace");
  await page.evaluate(() => localStorage.removeItem("qa.failWorkspaceConfig"));
  await page.getByRole("button", { name: "重新读取路径", exact: true }).click();
  await expect(page.locator(".onboarding-folder-picker small")).toHaveText("/tmp/qa-default");
  await expect(page.getByRole("button", { name: "完成设置", exact: true })).toBeEnabled();
});

test("workspace preview displays the standard default path", async ({ page }, testInfo) => {
  await page.addInitScript(() => localStorage.setItem("astro-locale", "zh"));
  await page.goto("/iframe.html?id=app-first-run-onboarding--workspace&viewMode=story");
  await expect(page.locator(".onboarding-folder-picker small")).toHaveText("~/.astro/workspace");
  await expect(page.getByText(/请确认工作空间路径/)).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("workspace-path.png"), fullPage: true, animations: "disabled" });
});

test("failed progress prevents completion and supports retry", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await verifyProvider(page);
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await page.evaluate(() => { (window as any).__failProgress = true; });
  await page.getByRole("button", { name: "完成设置", exact: true }).click();
  await expect(page.getByText(/初始化未能全部保存/)).toBeVisible();
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).completed)).toBe(false);
  await page.evaluate(() => { (window as any).__failProgress = false; });
  await page.getByRole("button", { name: "完成设置", exact: true }).click();
  await expect(page.getByRole("heading", { name: "一切准备就绪", exact: true })).toBeVisible();
});

test("reduced-motion narrow layout remains keyboard usable", async ({ page }) => {
  await installTransport(page);
  await page.emulateMedia({ reducedMotion: "reduce", contrast: "more" });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(RUNTIME_URL);
  await expect(page.getByRole("heading", { name: "先让这里更像你的工作空间" })).toBeVisible({ timeout: 30000 });
  expect(await page.locator(".onboarding-root").evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  await page.getByRole("button", { name: "继续", exact: true }).focus();
  await page.keyboard.press("Enter");
  await verifyProvider(page);
  await finishVerified(page);
  expect(await page.locator(".onboarding-root").evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  await page.getByRole("button", { name: "读懂一份文档 · 填入输入框", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".composer-input")).toHaveValue(/帮我快速读懂一份文档/);
});

test("hung provider times out without bypassing validation", async ({ page }) => {
  await installTransport(page);
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/__onboarding_mock/responses", async route => {
    await pending; await route.fulfill({ json: { ok: true, model: "qa-small" } }).catch(() => {});
  });
  await startProvider(page);
  await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
  await loadModels(page);
  const request = page.waitForRequest("**/__onboarding_mock/responses");
  await page.getByRole("button", { name: "测试", exact: true }).click();
  await request;
  await expect(page.getByRole("alert")).toContainText("连接测试超时", { timeout: 25000 });
  release();
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: /稍后连接|先进入 App/ })).toHaveCount(0);
});

test("provider list failure blocks setup but can be retried", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).step)).toBe("provider");
  await page.evaluate(() => localStorage.setItem("qa.failProviders", "true"));
  await page.reload();
  await expect(page.getByRole("alert")).toContainText("暂时无法连接服务");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: /稍后连接|先进入 App/ })).toHaveCount(0);
  await page.evaluate(() => localStorage.removeItem("qa.failProviders"));
  await page.getByRole("button", { name: "重试连接", exact: true }).click();
  await verifyProvider(page);
});

test("welcome and wizard reuse one moving brand element", async ({ page }) => {
  await page.goto("/iframe.html?id=app-first-run-onboarding--intro&viewMode=story");
  const brand = page.locator(".onboarding-brand-motion");
  await expect(brand).toBeVisible({ timeout: 30000 });
  const node = await brand.elementHandle();
  const bounds = await brand.boundingBox();
  await page.getByRole("button", { name: "跳过动画", exact: true }).click();
  await expect(brand).toHaveAttribute("data-phase", "header");
  await expect.poll(async () => (await brand.boundingBox())!.width).toBeLessThan(bounds!.width / 2);
  expect(await node!.evaluate(el => el.isConnected)).toBe(true);
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.getByRole("heading", { name: "为 Astro 接入思考能力" })).toBeVisible();
  expect(await node!.evaluate(el => el.isConnected)).toBe(true);
});

test("base_url is directly editable and edits invalidate model verification", async ({ page }, testInfo) => {
  await installTransport(page);
  await startProvider(page);
  const address = page.getByLabel("服务地址", { exact: true });
  await expect(address).toBeVisible();
  await expect(address).toBeEditable();
  await expect(address).toHaveValue("http://127.0.0.1/mock/v1");
  await expect(page.locator(".onboarding-provider-form details")).toHaveCount(0);
  await expect(page.getByText("高级连接设置", { exact: true })).toHaveCount(0);
  await verifyProvider(page);
  await address.fill("http://127.0.0.1/updated/v1");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "默认模型", exact: true })).toBeDisabled();
  await loadModels(page);
  await page.getByRole("button", { name: "测试", exact: true }).click();
  await expect(page.getByText("连接成功", { exact: true })).toBeVisible();
  const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("qa.providers")!).providers[0]);
  expect(saved.endpoint).toBe("http://127.0.0.1/updated/v1");
  await page.locator(".onboarding-provider-form").screenshot({
    path: testInfo.outputPath("visible-base-url.png"), animations: "disabled",
  });
});

for (const theme of ["light", "dark"] as const) {
  test(`model search filters names and IDs without choosing automatically · ${theme}`, async ({ page }, testInfo) => {
    await installTransport(page);
    await page.addInitScript(theme => localStorage.setItem("astro-theme-mode", theme), theme);
    await page.setViewportSize({ width: theme === "dark" ? 390 : 1100, height: 1000 });
    await page.route("**/__onboarding_mock/models", route => route.fulfill({
      json: { models: [
        { id: "vendor/rapid-v2", display_name: "Fast Chat" },
        { id: "vendor/reason", display_name: "Deep Think" },
        ...Array.from({ length: 80 }, (_, i) => ({ id: `vendor/model-${i}` })),
      ] },
    }));
    await startProvider(page);
    await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
    await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
    const picker = page.getByRole("button", { name: "默认模型", exact: true });
    const search = page.getByRole("combobox", { name: "搜索模型名称或 ID" });
    const popup = page.getByRole("dialog", { name: "默认模型", exact: true });
    await expect(search).toBeFocused();
    await expect(page.getByRole("option")).toHaveCount(82);
    await search.fill("  FAST CHAT  ");
    await expect(page.getByRole("option")).toHaveCount(1);
    await expect(page.getByRole("option")).toContainText("vendor/rapid-v2");
    await search.fill("VENDOR/RAPID");
    await expect(page.getByRole("option")).toHaveCount(1);
    await search.fill("no-such-model");
    await expect(page.getByRole("option")).toHaveCount(0);
    await expect(popup.getByRole("status")).toContainText("没有匹配的模型");
    await search.press("Enter");
    await expect(popup).toBeVisible();
    await expect(picker).toContainText("请选择默认模型");
    await search.fill("Think");
    await search.dispatchEvent("keydown", { key: "Enter", isComposing: true });
    await expect(popup).toBeVisible();
    await popup.screenshot({ path: testInfo.outputPath("model-search.png"), animations: "disabled" });
    await search.press("ArrowDown");
    await search.press("Enter");
    await expect(popup).toHaveCount(0);
    await expect(page.getByRole("button", { name: "测试", exact: true })).toBeFocused();
    await expect(picker).toContainText("Deep Think");
    await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
    if (await picker.getAttribute("aria-expanded") !== "true") await picker.click();
    await expect(search).toHaveValue("");
    await expect(page.getByRole("option")).toHaveCount(82);
    await search.fill("rapid");
    await search.press("Escape");
    await expect(popup).toHaveCount(0);
    await expect(picker).toBeFocused();
    await expect(picker).toContainText("Deep Think");
    if (await picker.getAttribute("aria-expanded") !== "true") await picker.click();
    await search.press("Tab");
    await expect(popup).toHaveCount(0);
  });
}

test("provider edits and blur never save until the explicit save/load action", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  const address = page.getByLabel("服务地址", { exact: true });
  await expect(page.locator(".onboarding-provider-form")).not.toContainText("base_url");
  const template = "https://YOUR_RESOURCE.services.ai.azure.com/openai/v1";
  const persistedBefore = await page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.endpoint);
  await address.fill(template);
  await address.press("Tab");
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.endpoint)).toBe(persistedBefore);
  await address.focus();
  await address.evaluate((input: HTMLInputElement) => {
    input.dataset.qaField = "stable";
    input.setSelectionRange(8, 21);
  });
  const savesBefore = await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => x.cmd === "save_onboarding_progress").length);
  // Slower than the old 180ms debounce: this used to trigger disk writes while typing.
  await address.pressSequentially("my-resource", { delay: 220 });
  await expect(address).toHaveValue("https://my-resource.services.ai.azure.com/openai/v1");
  await expect(address).toBeFocused();
  await expect(address).toHaveAttribute("data-qa-field", "stable");
  expect(await address.evaluate((input: HTMLInputElement) => [input.selectionStart, input.selectionEnd])).toEqual([19, 19]);
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => x.cmd === "save_onboarding_progress").length)).toBe(savesBefore);
  await address.press("ArrowLeft");
  await address.press("Backspace");
  await address.pressSequentially("X");
  await expect(address).toHaveValue("https://my-resourXe.services.ai.azure.com/openai/v1");
  await address.press("Tab");
  await page.getByLabel("API Key", { exact: true }).fill("qa-explicit-key");
  await page.getByLabel("API Key", { exact: true }).press("Tab");
  // Wait past the previous autosave debounce, including after focus leaves both fields.
  await page.waitForTimeout(1000);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.endpoint)).toBe(persistedBefore);
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => x.cmd === "save_onboarding_progress").length)).toBe(savesBefore);
  expect(await page.evaluate(() => (window as any).__onboardingCalls.filter((x: any) => ["save_provider", "set_provider_api_key", "list_provider_models", "verify_onboarding_provider"].includes(x.cmd)))).toEqual([]);
  await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "默认模型", exact: true })).toBeVisible();
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.endpoint)).toBe("https://my-resourXe.services.ai.azure.com/openai/v1");
  const submitted = await page.evaluate(() => ({
    calls: (window as any).__onboardingCalls.filter((x: any) => ["save_provider", "set_provider_api_key", "save_onboarding_progress", "list_provider_models"].includes(x.cmd)).slice(-4).map((x: any) => x.cmd),
    draft: localStorage.getItem("qa.state"),
  }));
  expect(submitted.calls).toEqual(["save_provider", "set_provider_api_key", "save_onboarding_progress", "list_provider_models"]);
  expect(submitted.draft).not.toContain("qa-explicit-key");
  await expect(page.getByLabel("API Key", { exact: true })).toHaveValue("");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
});

test("navigation cannot persist an edited but unsubmitted provider address", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await verifyProvider(page);
  const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.endpoint);
  await page.getByLabel("服务地址", { exact: true }).fill("https://not-submitted.example/v1");
  await page.getByRole("button", { name: "返回", exact: true }).click();
  await page.getByLabel("Agent 名称（可选）").fill("New name");
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.agent_name)).toBe("New name");
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.endpoint)).toBe(saved);
  await page.reload();
  await expect(page.getByRole("heading", { name: "先让这里更像你的工作空间" })).toBeVisible();
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.getByLabel("服务地址", { exact: true })).toHaveValue(saved);
});

test("explicitly saved credentials remain saved when model discovery fails", async ({ page }) => {
  await installTransport(page);
  await page.route("**/__onboarding_mock/models", route => route.fulfill({ json: { error: "network unavailable" } }));
  await startProvider(page);
  await page.getByLabel("服务地址", { exact: true }).fill("https://saved.example/v1");
  await page.getByLabel("API Key", { exact: true }).fill("qa-discovery-key");
  await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
  await expect(page.getByRole("alert", { name: "暂时无法连接服务" })).toBeVisible();
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.endpoint)).toBe("https://saved.example/v1");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  await page.reload();
  await expect(page.getByLabel("服务地址", { exact: true })).toHaveValue("https://saved.example/v1");
  await expect(page.getByLabel("API Key", { exact: true })).toHaveValue("");
});

test("credential URL is not saved and cannot be tested", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await page.getByLabel("服务地址", { exact: true }).fill("https://example.com?api_key=qa-must-not-persist");
  await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
  await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("暂时无法连接服务");
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem("qa.state")!).draft.endpoint)).toBe("");
  expect(await page.evaluate(() => localStorage.getItem("qa.state"))).not.toContain("qa-must-not-persist");
});

for (const theme of ["light", "dark"] as const) {
  test(`provider picker reuses the shared menu · ${theme}`, async ({ page }, testInfo) => {
    await page.addInitScript(theme => {
      localStorage.setItem("astro-theme-mode", theme);
      localStorage.setItem("astro-locale", "zh");
    }, theme);
    await page.setViewportSize({ width: theme === "dark" ? 390 : 1100, height: 850 });
    await page.goto("/iframe.html?id=app-first-run-onboarding--provider&viewMode=story");
    const testButton = page.getByRole("button", { name: "测试", exact: true });
    await expect(testButton).toBeVisible();
    await expect(testButton.locator("svg.lucide-zap")).toHaveCount(1);
    const picker = page.getByRole("button", { name: "模型服务", exact: true });
    await expect(picker).toBeVisible({ timeout: 30000 });
    await expect(page.locator(".onboarding-provider-form select")).toHaveCount(0);
    if (await picker.getAttribute("aria-expanded") !== "true") await picker.click();
    const menu = page.getByRole("listbox", { name: "模型服务", exact: true });
    await expect(menu).toBeVisible();
    await expect(page.getByRole("option", { name: "OpenAI", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(page.getByRole("option", { name: "Google Gemini", exact: true }).locator("svg")).toHaveCount(1);
    const placement = await menu.evaluate(element => {
      const rect = element.getBoundingClientRect();
      const root = document.querySelector(".onboarding-root")!;
      return { left: rect.left, right: rect.right, viewport: innerWidth,
        onTop: Number(getComputedStyle(element).zIndex) > Number(getComputedStyle(root).zIndex),
        portaled: element.parentElement === document.body };
    });
    expect(placement.onTop).toBe(true);
    expect(placement.portaled).toBe(true);
    expect(placement.left).toBeGreaterThanOrEqual(0);
    expect(placement.right).toBeLessThanOrEqual(placement.viewport);
    await page.getByRole("option", { name: "Google Gemini", exact: true }).click();
    await expect(menu).toHaveCount(0);
    await expect(picker).toContainText("Google Gemini");
    await expect(page.getByRole("button", { name: "默认模型", exact: true })).toBeDisabled();
    await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
    await expect(page.getByRole("button", { name: "默认模型", exact: true })).toBeEnabled();
    await expect(page.getByRole("button", { name: "默认模型", exact: true })).toContainText("gemini-3.1-pro");
    await chooseModel(page, "gemini-3.1-pro");
    await page.getByRole("button", { name: "默认模型", exact: true }).click();
    await page.screenshot({ path: testInfo.outputPath("model-menu.png"), fullPage: true, animations: "disabled" });
    await page.keyboard.press("Escape");
    await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
    await picker.press("ArrowDown");
    await expect(menu).toBeVisible();
    await picker.press("Home");
    await picker.press("Enter");
    await expect(picker).toContainText("OpenAI");
    await expect(menu).toHaveCount(0);
    await picker.press("ArrowDown");
    await picker.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(picker).toBeFocused();
  });
}

test("loading models requires an explicit choice and key changes reset it", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  const picker = page.getByRole("button", { name: "默认模型", exact: true });
  await expect(picker).toBeDisabled();
  await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
  await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
  await expect(picker).toBeEnabled();
  await expect(picker).toContainText("请选择默认模型");
  await expect(page.getByRole("button", { name: "测试", exact: true })).toBeDisabled();
  if (await picker.getAttribute("aria-expanded") !== "true") await picker.click();
  await expect(page.getByRole("option")).toHaveCount(2);
  await expect(page.getByRole("option", { name: "text-embedding-3-small" })).toHaveCount(0);
  await page.getByRole("option", { name: "qa-alt", exact: true }).click();
  const calls = await page.evaluate(() => (window as any).__onboardingCalls);
  expect(calls.filter((x: any) => x.cmd === "list_provider_models")[0].args.refresh).toBe(true);
  expect(calls.filter((x: any) => x.cmd === "verify_onboarding_provider")).toEqual([]);
  await page.getByLabel("API Key", { exact: true }).fill("qa-replacement-key");
  await expect(picker).toBeDisabled();
  await expect(picker).toContainText("请选择默认模型");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
});

for (const result of [
  { models: [] },
  { error: "401 invalid api key qa-model-list-secret" },
]) {
  test(`failed or empty model lists cannot use a built-in default: ${"error" in result ? "error" : "empty"}`, async ({ page }) => {
    await installTransport(page);
    await page.route("**/__onboarding_mock/models", route => route.fulfill({ json: result }));
    await startProvider(page);
    await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
    await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
    await expect(page.getByRole("alert")).toBeVisible();
    await expect(page.locator("body")).not.toContainText("qa-model-list-secret");
    await expect(page.getByRole("button", { name: "默认模型", exact: true })).toBeDisabled();
    await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
    await page.route("**/__onboarding_mock/models", route => route.fulfill({ json: { models: [{ id: "qa-small" }] } }));
    await page.getByRole("button", { name: "重试连接", exact: true }).click();
    await expect(page.getByRole("button", { name: "默认模型", exact: true })).toBeEnabled();
    await expect(page.getByRole("button", { name: "默认模型", exact: true })).toContainText("请选择默认模型");
  });
}

test("cancelled model loading ignores late results", async ({ page }, testInfo) => {
  await installTransport(page);
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/__onboarding_mock/models", async route => {
    await pending;
    await route.fulfill({ json: { models: [{ id: "qa-stale" }] } });
  });
  await startProvider(page);
  await page.getByLabel("API Key", { exact: true }).fill("qa-placeholder-key");
  const request = page.waitForRequest("**/__onboarding_mock/models");
  const loadButton = page.locator(".onboarding-load-models");
  const before = (await loadButton.boundingBox())!;
  await page.getByRole("button", { name: "保存密钥并获取模型", exact: true }).click();
  await request;
  await expect(loadButton).toHaveAccessibleName("取消等待");
  await expect(loadButton).toBeEnabled();
  await expect(loadButton.locator("svg.lucide-circle-x")).toHaveCount(1);
  await expect(page.locator(".onboarding-test-button")).toHaveAccessibleName("测试");
  await expect(page.locator(".onboarding-test-button")).toBeDisabled();
  await expect(page.getByRole("button", { name: "取消等待", exact: true })).toHaveCount(1);
  const during = (await loadButton.boundingBox())!;
  for (const key of ["x", "y", "width", "height"] as const) expect(Math.abs(during[key] - before[key])).toBeLessThan(1);
  await page.screenshot({ path: testInfo.outputPath("models-inline-cancel.png") });
  await loadButton.focus();
  await loadButton.press("Enter");
  await expect(loadButton).toHaveAccessibleName("保存密钥并获取模型");
  await expect(loadButton).toBeFocused();
  await page.getByLabel("API Key", { exact: true }).fill("qa-replacement-key");
  const response = page.waitForResponse("**/__onboarding_mock/models");
  release(); await response;
  await expect(page.getByRole("button", { name: "默认模型", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
});

test("the test action becomes retest on success and retry on failure", async ({ page }) => {
  await installTransport(page);
  await startProvider(page);
  await verifyProvider(page);
  const button = page.locator(".onboarding-test-button");
  await expect(button).toHaveAccessibleName("重新测试");
  await expect(button.locator("svg.lucide-zap")).toHaveCount(1);
  await expect(page.locator(".onboarding-test-action").getByRole("status")).toContainText("连接成功");
  await page.route("**/__onboarding_mock/responses", route => route.fulfill({ json: { ok: false, message: "invalid api key" } }));
  await button.click();
  await expect(button).toHaveAccessibleName("重试");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeDisabled();
  await page.route("**/__onboarding_mock/responses", route => route.fulfill({ json: { ok: true, model: "qa-small", latency_ms: 1 } }));
  await button.click();
  await expect(button).toHaveAccessibleName("重新测试");
  await expect(page.getByRole("button", { name: "继续", exact: true })).toBeEnabled();
});

test("settings show only added providers and can remove the last account", async ({ page }) => {
  await installTransport(page, "existing");
  await page.goto(RUNTIME_URL);
  await page.getByRole("button", { name: "连接模型", exact: true }).click();
  await expect(page.getByText("尚未添加模型服务，请从下方添加提供商。", { exact: true })).toBeVisible();
  await expect(page.locator(".providers-list-item")).toHaveCount(0);
  await page.locator(".providers-add-btn").click();
  await expect(page.locator(".providers-list-item")).toHaveCount(1);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("qa.providers")!).providers[0].has_api_key)).toBe(false);
  await page.getByRole("button", { name: "更多操作", exact: true }).click();
  await page.getByRole("menuitem", { name: "删除", exact: true }).click();
  await page.getByRole("dialog").getByRole("button", { name: "删除", exact: true }).click();
  await expect(page.getByText("尚未添加模型服务，请从下方添加提供商。", { exact: true })).toBeVisible();
  await page.reload();
  await page.getByRole("button", { name: "连接模型", exact: true }).click();
  await expect(page.locator(".providers-list-item")).toHaveCount(0);
});
