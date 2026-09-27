import { expect, test, type Page } from "@playwright/test";

// 本会话额外权限（`request_permissions` 批准后）：权限菜单里可见，并能一键撤销。
const SESSION_KEY = "astro.chat.session";
const SESSION_ID = "sess-permission-grants";

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
      localStorage.setItem("astro-glass-intensity", "35");
      localStorage.setItem("astro-interface-material", "glass");
      localStorage.setItem("astro-shell-color-style", "dynamic");
      localStorage.setItem(
        "astro-shell-color-prefs.v2",
        JSON.stringify({ style: "dynamic", dynamicSeed: "timeline-icons" }),
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
      localStorage.setItem(
        sessionKey,
        JSON.stringify({ sessionId, messages: [], updatedAt: Date.now() }),
      );

      let serial = 0;
      const callbacks = new Map<number, unknown>();
      const timeline = [
        {
          type: "reasoning",
          id: "r-1",
          text: "先想一下",
          at: 1_700_000_000_000,
          durationSec: 3.2,
        },
        { type: "activity", id: "call_1", at: 1_700_000_003_200 },
        { type: "activity", id: "call_2", at: 1_700_000_005_000 },
        {
          type: "reasoning",
          id: "r-2",
          text: "再核对",
          at: 1_700_000_007_000,
          durationSec: 2.1,
        },
        { type: "activity", id: "call_3", at: 1_700_000_008_500 },
        { type: "text", id: "txt-1", text: "已经跑完了。", at: 1_700_000_009_000 },
      ];
      const tool = (id: string, name: string, args: string, output: string) => [
        {
          id: `${id}-call`,
          timestamp: 1_700_000_004,
          item: { type: "function_call", call_id: id, name, arguments: args },
        },
        {
          id: `${id}-out`,
          timestamp: 1_700_000_005,
          item: { type: "function_call_output", call_id: id, name, output },
        },
      ];

      (window as unknown as { __permissionCalls: unknown[] }).__permissionCalls = [];
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
                  workspace_dir: "/tmp/qa-timeline-icons",
                  memory_dir: "/tmp/qa-timeline-icons",
                  grpc_address: "",
                };
              case "get_default_workspace_path":
                return "/tmp/qa-timeline-icons";
              case "get_providers_state":
                return {
                  providers: [
                    {
                      id: "qa",
                      kind: "openai",
                      display_name: "QA",
                      model: "qa",
                      enabled: true,
                      has_api_key: true,
                      supports_responses_api: true,
                    },
                  ],
                  provider_templates: [],
                  active_provider_id: "qa",
                };
              case "list_projects":
                return [
                  {
                    id: "default",
                    name: "主空间",
                    roots: ["/tmp/qa-timeline-icons"],
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
              case "get_session_permission_grants":
                (window as unknown as { __permissionCalls: unknown[] }).__permissionCalls.push({
                  command: "get_session_permission_grants",
                });
                return {
                  workspaceWrite: true,
                  writableRoots: ["/tmp/shared-out"],
                };
              case "revoke_session_permission_grants":
                (window as unknown as { __permissionCalls: unknown[] }).__permissionCalls.push({
                  command: "revoke_session_permission_grants",
                });
                return { workspaceWrite: false, writableRoots: [] };
              case "get_chat_history":
                return {
                  sessionId,
                  items: [
                    {
                      id: "u1",
                      timestamp: 1_700_000_000,
                      item: {
                        type: "message",
                        role: "user",
                        content: [{ type: "input_text", text: "运行并检查一下" }],
                      },
                    },
                    ...tool("call_1", "exec_command", '{"cmd":"ls"}', "a.txt"),
                    ...tool("call_2", "read_file", '{"path":"a.txt"}', "hello"),
                    ...tool("call_3", "exec_command", '{"cmd":"pwd"}', "/tmp"),
                    {
                      id: "a1",
                      timestamp: 1_700_000_010,
                      item: {
                        type: "message",
                        role: "assistant",
                        content: [{ type: "output_text", text: "已经跑完了。" }],
                        internal_chat_message_metadata_passthrough: {
                          astro_timeline_v1: timeline,
                        },
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


test("session permission grants are visible and revocable from the policy menu", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 900 });
  await boot(page);

  // 角标：不用打开菜单就能看到本会话有额外权限。
  await expect(page.locator(".composer-policy-grants-badge")).toHaveText("+1");

  // 打开菜单看细节，并撤销。
  await page.locator(".composer-policy-pill").click();
  const grants = page.locator(".composer-policy-grants");
  await expect(grants).toBeVisible();
  await expect(grants).toContainText("本会话额外权限");
  await expect(grants.locator("li")).toHaveText(["/tmp/shared-out"]);

  await grants.getByRole("button", { name: "撤销本会话额外权限" }).click();
  await expect(page.locator(".composer-policy-grants")).toHaveCount(0);
  await expect(page.locator(".composer-policy-grants-badge")).toHaveCount(0);
  const calls = await page.evaluate(
    () => (window as unknown as { __permissionCalls: unknown[] }).__permissionCalls,
  );
  expect(
    calls.filter(
      (call: { command?: string }) =>
        call.command === "revoke_session_permission_grants",
    ),
  ).toHaveLength(1);
});
