import { expect, test, type Page } from "@playwright/test";

// Production App and CSS; only the native transport is replaced with fixtures.
// Guards: switching sessions must keep each session's context usage snapshot
// (ring + layered breakdown) instead of falling back to the empty state.

const USAGE_MAP_KEY = "astro.chat.contextUsageBySession";

const usageSnapshot = {
  contextWindow: 1_000_000,
  totalTokens: 20_300,
  estimatedTotalTokens: 20_300,
  source: "provider_reported",
  segments: [
    { id: "conversation", tokens: 6800, count: 12 },
    { id: "tools", tokens: 5300, count: 24 },
    { id: "system", tokens: 672 },
  ],
  latestUsage: {
    inputTokens: 19_000,
    uncachedInputTokens: 5_400,
    outputTokens: 1300,
    totalTokens: 20_300,
    cacheReadTokens: 13_600,
    cacheWriteTokens: 0,
    reasoningTokens: 223,
    cacheReadReported: true,
    cacheWriteReported: false,
    reasoningReported: true,
  },
  updatedAt: 1_700_000_000_000,
};

const sessions = [
  {
    sessionId: "sess-a",
    source: "tauri",
    summary: "会话 A",
    createdAt: new Date(Date.now() - 60_000).toISOString(),
    endReason: null,
    archivedAt: null,
    pinnedAt: null,
  },
  {
    sessionId: "sess-b",
    source: "tauri",
    summary: "会话 B",
    createdAt: new Date(Date.now() - 120_000).toISOString(),
    endReason: null,
    archivedAt: null,
    pinnedAt: null,
  },
];

function historyFor(sessionId: string) {
  return {
    sessionId,
    items: [
      {
        id: `${sessionId}-u1`,
        timestamp: 1_700_000_000,
        item: {
          type: "message",
          role: "user",
          content: [{ type: "input_text", text: `${sessionId} 的问题` }],
        },
      },
      {
        id: `${sessionId}-a1`,
        timestamp: 1_700_000_001,
        item: {
          type: "message",
          role: "assistant",
          content: [{ type: "output_text", text: `${sessionId} 的回答` }],
        },
      },
    ],
    endReason: null,
  };
}

async function boot(page: Page) {
  await page.route("**/*", (route) => {
    const { hostname } = new URL(route.request().url());
    return ["localhost", "127.0.0.1"].includes(hostname)
      ? route.continue()
      : route.abort();
  });
  await page.addInitScript(
    ({ mapKey, snapshot, sessionList }) => {
      localStorage.setItem("astro-theme-mode", "light");
      localStorage.setItem("astro-locale", "zh");
      localStorage.setItem("astro.sidebarLabels", "1");
      localStorage.setItem("astro.sidebarPinned", "1");
      localStorage.removeItem(mapKey);
      localStorage.setItem("astro-glass-intensity", "35");
      localStorage.setItem("astro-interface-material", "glass");
      localStorage.setItem("astro-shell-color-style", "dynamic");
      localStorage.setItem(
        "astro-shell-color-prefs.v2",
        JSON.stringify({
          style: "dynamic",
          gradient: {
            id: "rose",
            primary: { color: "#db2777", x: 16, y: 10 },
            secondary: { color: "#fb7185", x: 86, y: 24 },
            extras: [],
          },
          dynamicSeed: "context-usage-session",
        }),
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
      const listeners = new Map<number, { event: string; handler: number }>();
      const emit = (event: string, payload: unknown) => {
        for (const [id, listener] of listeners) {
          if (listener.event !== event) continue;
          callbacks.get(listener.handler)?.({
            id,
            event,
            payload,
          });
        }
      };
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
          invoke: async (cmd: string, args: unknown) => {
            const payload = args as {
              sessionId?: string;
              request?: { sessionId?: string };
            };
            const request = {
              sessionId: payload?.sessionId ?? payload?.request?.sessionId,
            };
            switch (cmd) {
              case "plugin:event|listen": {
                const id = ++serial;
                const listenArgs = args as { event: string; handler: number };
                listeners.set(id, {
                  event: listenArgs.event,
                  handler: listenArgs.handler,
                });
                return id;
              }
              case "plugin:event|unlisten": {
                const listenArgs = args as { eventId?: number };
                if (listenArgs.eventId != null)
                  listeners.delete(listenArgs.eventId);
                return null;
              }
              case "start_chat": {
                const req = (
                  args as { request?: { sessionId?: string | null } }
                )?.request;
                const sid = req?.sessionId ?? "sess-a";
                setTimeout(() => {
                  emit(`chat_stream_${sid}`, {
                    type: "token",
                    content: "收到",
                  });
                  emit(`chat_stream_${sid}`, {
                    type: "context_usage",
                    context_window: snapshot.contextWindow,
                    total_tokens: snapshot.totalTokens,
                    estimated_total_tokens: snapshot.estimatedTotalTokens,
                    source: snapshot.source,
                    segments: snapshot.segments,
                    latest_usage: {
                      input_tokens: snapshot.latestUsage.inputTokens,
                      output_tokens: snapshot.latestUsage.outputTokens,
                      total_tokens: snapshot.latestUsage.totalTokens,
                      cache_read_tokens: snapshot.latestUsage.cacheReadTokens,
                      reasoning_tokens: snapshot.latestUsage.reasoningTokens,
                      cache_read_reported: true,
                      reasoning_reported: true,
                    },
                    updated_at: snapshot.updatedAt,
                  });
                  emit(`chat_stream_${sid}`, { type: "done" });
                }, 60);
                return sid;
              }
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
                  workspace_dir: "/tmp/qa-session",
                  memory_dir: "/tmp/qa-session",
                  grpc_address: "",
                };
              case "get_default_workspace_path":
                return "/tmp/qa-session";
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
                    roots: ["/tmp/qa-session"],
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
                return [];
              case "list_sessions":
                return sessionList;
              case "get_context_usage": {
                if (request.sessionId !== "sess-a") return null;
                // Rust 侧返回持久化的 ContextUsageEvent（snake_case）。
                return {
                  turn_id: "turn-persisted",
                  context_window: snapshot.contextWindow,
                  total_tokens: snapshot.totalTokens,
                  estimated_total_tokens: snapshot.estimatedTotalTokens,
                  source: snapshot.source,
                  latest_usage: {
                    input_tokens: snapshot.latestUsage.inputTokens,
                    uncached_input_tokens: 5_400,
                    output_tokens: snapshot.latestUsage.outputTokens,
                    total_tokens: snapshot.latestUsage.totalTokens,
                    cache_read_tokens: snapshot.latestUsage.cacheReadTokens,
                    cache_write_tokens: 0,
                    reasoning_tokens: snapshot.latestUsage.reasoningTokens,
                    cache_read_reported: true,
                    cache_write_reported: false,
                    reasoning_reported: true,
                  },
                  segments: snapshot.segments,
                  updated_at: snapshot.updatedAt,
                  recommend_compact: false,
                };
              }
              case "get_chat_history": {
                const id = request?.sessionId ?? "sess-a";
                return {
                  sessionId: id,
                  items: [
                    {
                      id: `${id}-u1`,
                      timestamp: 1_700_000_000,
                      item: {
                        type: "message",
                        role: "user",
                        content: [
                          { type: "input_text", text: `${id} 的问题` },
                        ],
                      },
                    },
                    {
                      id: `${id}-a1`,
                      timestamp: 1_700_000_001,
                      item: {
                        type: "message",
                        role: "assistant",
                        content: [
                          { type: "output_text", text: `${id} 的回答` },
                        ],
                      },
                    },
                  ],
                  endReason: null,
                };
              }
              default:
                if (cmd.startsWith("get_")) return null;
                if (cmd.startsWith("list_")) return [];
                return null;
            }
          },
        },
      });
    },
    { mapKey: USAGE_MAP_KEY, snapshot: usageSnapshot, sessionList: sessions },
  );
  await page.goto(
    "/iframe.html?id=app-onboarding-runtime--strict-native-transport&viewMode=story&globals=theme:light",
  );
  await expect(page.locator(".sidebar")).toBeVisible({ timeout: 30000 });
}

async function openSession(page: Page, summary: string) {
  await page
    .locator(".sidebar-session-item", { hasText: summary })
    .first()
    .click();
  await expect(
    page.locator(".conversation-title"),
  ).toContainText(summary, { timeout: 15000 });
}

async function usageState(page: Page) {
  return page.evaluate((mapKey) => {
    const button = document.querySelector(".composer-context-btn");
    const raw = localStorage.getItem(mapKey);
    const map = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
    return {
      ringValue: Boolean(
        document.querySelector(".composer-context-ring-value"),
      ),
      buttonTitle: button?.getAttribute("title") ?? null,
      mapKeys: Object.keys(map),
    };
  }, USAGE_MAP_KEY);
}

test("context usage survives switching sessions and coming back", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await boot(page);

  await openSession(page, "会话 A");
  await page.locator(".composer-input").fill("你好");
  await page.locator(".send-btn--round").last().click();
  await expect
    .poll(async () => (await usageState(page)).ringValue, { timeout: 15000 })
    .toBe(true);
  const afterA = await usageState(page);
  expect(afterA.ringValue).toBe(true);
  expect(afterA.mapKeys).toContain("sess-a");

  await openSession(page, "会话 B");
  const afterB = await usageState(page);
  expect(afterB.ringValue).toBe(false);

  await openSession(page, "会话 A");
  const backToA = await usageState(page);
  expect(backToA.ringValue).toBe(true);
  expect(backToA.buttonTitle).toBe(afterA.buttonTitle);
});

test("context usage hydrates from the persisted rollout when the local cache is empty", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await boot(page);

  // 清掉本地按会话缓存：占用与分层只能来自 rollout 持久化快照。
  await page.evaluate(
    (mapKey) => localStorage.removeItem(mapKey),
    USAGE_MAP_KEY,
  );

  await openSession(page, "会话 B");
  expect((await usageState(page)).ringValue).toBe(false);

  await openSession(page, "会话 A");
  await expect
    .poll(async () => (await usageState(page)).ringValue, { timeout: 15000 })
    .toBe(true);
  expect((await usageState(page)).mapKeys).toContain("sess-a");

  await page.locator(".composer-context-btn").hover();
  const popover = page.locator(".ctx-usage-popover");
  await expect(popover).toBeVisible();
  await expect(popover.locator(".ctx-usage-legend-row")).toHaveCount(3);
  await expect(popover).toContainText("会话消息");
  await expect(popover).toContainText("工具定义");
});
