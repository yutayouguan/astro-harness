import { expect, test, type Page } from "@playwright/test";

// 本地快照只是首屏缓存：崩溃残留、压缩/回滚或其它窗口写入都可能让它落后，
// 启动后必须用 DB 历史校正，而不是一直显示旧消息。
const SESSION_KEY = "astro.chat.session";
const SESSION_ID = "sess-restore";

async function boot(page: Page) {
  await page.route("**/*", (route) => {
    const { hostname } = new URL(route.request().url());
    return ["localhost", "127.0.0.1"].includes(hostname)
      ? route.continue()
      : route.abort();
  });
  await page.addInitScript(
    ({ sessionKey, sessionId }) => {
      localStorage.setItem("astro-theme-mode", "light");
      localStorage.setItem("astro-locale", "zh");
      localStorage.setItem("astro.sidebarLabels", "1");
      localStorage.setItem("astro.sidebarPinned", "1");
      localStorage.setItem("astro-glass-intensity", "35");
      localStorage.setItem("astro-interface-material", "glass");
      localStorage.setItem("astro-shell-color-style", "dynamic");
      localStorage.setItem(
        "astro-shell-color-prefs.v2",
        JSON.stringify({ style: "dynamic", dynamicSeed: "session-restore" }),
      );
      localStorage.setItem(
        "astro-wallpaper-prefs.v1",
        JSON.stringify({
          mode: "color",
          current: null,
          recent: [],
          fit: "cover",
          shade: 18,
          blur: 0,
          adaptiveColor: false,
          followSystemWallpaper: false,
        }),
      );
      // 本地快照只剩两条旧消息（模拟崩溃残留 / 其它窗口写入）。
      localStorage.setItem(
        sessionKey,
        JSON.stringify({
          sessionId,
          messages: [
            { id: "cache-u1", role: "user", content: "缓存里的旧问题" },
            { id: "cache-a1", role: "assistant", content: "缓存里的旧回答" },
          ],
          updatedAt: Date.now(),
        }),
      );

      const provider = {
        id: "qa",
        kind: "openai",
        display_name: "QA",
        model: "qa",
        enabled: true,
        has_api_key: true,
        supports_responses_api: true,
      };
      let serial = 0;
      const callbacks = new Map();
      Object.assign(window, {
        isTauri: true,
        __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
        __TAURI_INTERNALS__: {
          metadata: {
            currentWindow: { label: "main" },
            currentWebview: { label: "main", windowLabel: "main" },
          },
          transformCallback: (fn: unknown) => {
            callbacks.set(++serial, fn);
            return serial;
          },
          unregisterCallback: (id: number) => callbacks.delete(id),
          convertFileSrc: (path: string) => path,
          invoke: async (cmd: string) => {
            switch (cmd) {
              case "plugin:event|listen":
                return ++serial;
              case "get_onboarding_state":
                return {
                  version: 1,
                  completed: true,
                  should_show: false,
                  step: "complete",
                };
              case "get_interface_tour_state":
                return { resolved_version: 1 };
              case "get_config":
                return {
                  agents: [{ id: "default", name: "Astro" }],
                  active_agent_id: "default",
                  workspace_dir: "/tmp/qa-restore",
                  memory_dir: "/tmp/qa-restore",
                  grpc_address: "",
                };
              case "get_default_workspace_path":
                return "/tmp/qa-restore";
              case "get_providers_state":
                return {
                  providers: [provider],
                  provider_templates: [],
                  active_provider_id: "qa",
                };
              case "list_projects":
                return [
                  {
                    id: "default",
                    name: "主空间",
                    roots: ["/tmp/qa-restore"],
                    position: 0,
                  },
                ];
              case "get_permission_settings":
                return {
                  preset: "ask_for_approval",
                  sandboxHealth: { status: "available", backend: "qa" },
                };
              case "get_cached_provider_models":
                return { models: [], latency_ms: 0, source: "qa" };
              case "get_mcp_servers":
              case "get_agent_tools":
              case "list_sessions":
                return [];
              case "get_context_usage":
                return null;
              case "get_chat_history":
                // DB 里比本地快照多一条：用户发过第二条消息。
                return {
                  sessionId,
                  items: [
                    {
                      id: "db-u1",
                      timestamp: 1_700_000_000,
                      item: {
                        type: "message",
                        role: "user",
                        content: [{ type: "input_text", text: "缓存里的旧问题" }],
                      },
                    },
                    {
                      id: "db-a1",
                      timestamp: 1_700_000_001,
                      item: {
                        type: "message",
                        role: "assistant",
                        content: [{ type: "output_text", text: "缓存里的旧回答" }],
                      },
                    },
                    {
                      id: "db-u2",
                      timestamp: 1_700_000_002,
                      item: {
                        type: "message",
                        role: "user",
                        content: [{ type: "input_text", text: "DB 里的新问题" }],
                      },
                    },
                  ],
                  endReason: null,
                };
              default:
                if (cmd.startsWith("get_")) return null;
                if (cmd.startsWith("list_")) return [];
                return null;
            }
          },
        },
      });
    },
    { sessionKey: SESSION_KEY, sessionId: SESSION_ID },
  );
  await page.goto(
    "/iframe.html?id=app-onboarding-runtime--strict-native-transport&viewMode=story&globals=theme:light",
  );
  await expect(page.locator(".sidebar")).toBeVisible({ timeout: 30000 });
}

test("a stale local snapshot is reconciled with the database on boot", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await boot(page);

  // 首屏可以先用快照，但 DB 回读完成后必须以 DB 为准（多出的第三条消息要出现）。
  await expect(page.locator(".msg-row")).toHaveCount(3, { timeout: 15000 });
  await expect(page.locator(".message-list")).toContainText("DB 里的新问题");
});
