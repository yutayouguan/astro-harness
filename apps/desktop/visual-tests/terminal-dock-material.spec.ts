import { expect, test, type Page } from "@playwright/test";

// Production App and CSS; only the native transport is faked. Guards the terminal
// reading field: the xterm area is a transparent pane over the dock's frosted
// glass (no extra white plane), and the dock header carries no white halo overlay.

const TERMINAL_TOGGLE = "打开或折叠终端（⌘/Ctrl+J）";

async function boot(page: Page, theme: string) {
  await page.route("**/*", (route) => {
    const { hostname } = new URL(route.request().url());
    return ["localhost", "127.0.0.1"].includes(hostname)
      ? route.continue()
      : route.abort();
  });
  await page.addInitScript(
    ({ theme }) => {
      localStorage.setItem("astro-theme-mode", theme);
      localStorage.setItem("astro-locale", "zh");
      localStorage.setItem("astro.sidebarLabels", "1");
      localStorage.setItem("astro.sidebarPinned", "1");
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
          dynamicSeed: "197f416e-49ff-4080-8b84-2510030f4b62",
        }),
      );
      localStorage.setItem(
        "astro-wallpaper-prefs.v1",
        JSON.stringify({
          mode: "wallpaper",
          current: {
            id: "qa-wallpaper",
            path: "/mcp-icons/anthropic.svg",
            name: "QA",
            source: "upload",
            width: 1536,
            height: 1024,
            createdAt: "2026-09-15T00:00:00Z",
            recommendedTheme: "dark",
          },
          recent: [],
          fit: "cover",
          shade: 18,
          blur: 0,
          adaptiveColor: true,
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
      const terminalData = Array.from(
        new TextEncoder().encode("AI ~/.astro/workspace % "),
      );
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
            const request = (args as { request?: { cursor: number } })?.request;
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
                  workspace_dir: "/tmp/qa-terminal",
                  memory_dir: "/tmp/qa-terminal",
                  grpc_address: "",
                };
              case "get_default_workspace_path":
                return "/tmp/qa-terminal";
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
                    roots: ["/tmp/qa-terminal"],
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
              case "terminal_open":
                return {
                  id: 1,
                  scope: "user",
                  cwd: "/tmp/qa-terminal",
                  running: true,
                  exitCode: null,
                  baseCursor: 0,
                  endCursor: 0,
                };
              case "terminal_resize":
              case "terminal_write":
              case "terminal_close":
                return null;
              case "terminal_read": {
                const cursor = request?.cursor ?? 0;
                if (cursor < terminalData.length) {
                  return {
                    id: 1,
                    data: terminalData.slice(cursor),
                    nextCursor: terminalData.length,
                    dropped: false,
                    running: true,
                  };
                }
                // Idle PTY: a long poll, not a busy loop.
                return new Promise(() => {});
              }
              default:
                if (cmd.startsWith("get_"))
                  throw Error("QA read unavailable: " + cmd);
                if (cmd.startsWith("list_")) return [];
                return null;
            }
          },
        },
      });
    },
    { theme },
  );
  await page.goto(
    `/iframe.html?id=app-onboarding-runtime--strict-native-transport&viewMode=story&globals=theme:${theme}`,
  );
  await expect(page.locator(".sidebar")).toBeVisible({ timeout: 30000 });
}

for (const theme of ["light", "dark"]) {
  test(`${theme}: terminal content reads through the dock frost under a wallpaper`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await boot(page, theme);
    await page.getByRole("button", { name: TERMINAL_TOGGLE }).click();
    await expect(page.locator(".terminal-dock")).toHaveClass(/is-open/, {
      timeout: 15000,
    });
    await expect(page.locator(".terminal-dock-screen .xterm")).toBeVisible({
      timeout: 15000,
    });

    const measured = await page.evaluate(() => {
      const dock = document.querySelector(".terminal-dock")!;
      const screen = document.querySelector(".terminal-dock-screen")!;
      const header = document.querySelector(".terminal-dock-header")!;
      const sidebar = document.querySelector(".sidebar")!;
      const rgb = (value: string) => {
        const match = value.match(/[\d.]+/g)?.map(Number) ?? [];
        return { r: match[0], g: match[1], b: match[2], a: match[3] ?? 1 };
      };
      const resolve = (host: Element, variable: string) => {
        const probe = document.createElement("span");
        probe.style.backgroundColor = `var(${variable})`;
        host.appendChild(probe);
        const value = getComputedStyle(probe).backgroundColor;
        probe.remove();
        return value;
      };
      return {
        screen: rgb(getComputedStyle(screen).backgroundColor),
        sidebarPlane: rgb(resolve(sidebar, "--sidebar-bg")),
        dockBackdrop: getComputedStyle(dock).backdropFilter,
        headerImage: getComputedStyle(header).backgroundImage,
        xterm: getComputedStyle(
          screen.querySelector(".xterm") as Element,
        ).backgroundColor,
      };
    });

    // 终端不自带偏白底板：文字直接读 dock 的磨砂玻璃。
    expect(measured.screen.a).toBe(0);
    expect(measured.screen).not.toEqual(measured.sidebarPlane);
    expect(measured.dockBackdrop).toMatch(/blur/);
    // xterm keeps painting transparent rows on top of that glass.
    expect(measured.xterm).toMatch(/rgba\(0, 0, 0, 0\)|transparent/);
    // No white gradient halo on the dock header.
    expect(measured.headerImage).toBe("none");

    await page.screenshot({
      path: testInfo.outputPath("terminal-dock-material.png"),
    });
  });
}
