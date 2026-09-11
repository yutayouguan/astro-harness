import { expect, test, type Page } from "@playwright/test";

// Production App and CSS; only the native transport is replaced with local fixtures.
async function boot(page: Page, theme: string, colorStyle: string) {
  await page.route("**/*", (route) => {
    const { hostname } = new URL(route.request().url());
    return ["localhost", "127.0.0.1"].includes(hostname)
      ? route.continue()
      : route.abort();
  });
  await page.addInitScript(
    ({ theme, colorStyle }) => {
      localStorage.setItem("astro-theme-mode", theme);
      localStorage.setItem("astro-locale", "zh");
      localStorage.setItem("astro.sidebarLabels", "1");
      localStorage.setItem(
        "astro-shell-color-prefs.v2",
        JSON.stringify({
          style: colorStyle,
          dynamicSeed: "sidebar-material-regression",
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
                  workspace_dir: "/tmp/qa-sidebar",
                  memory_dir: "/tmp/qa-sidebar",
                  grpc_address: "",
                };
              case "get_default_workspace_path":
                return "/tmp/qa-sidebar";
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
                    roots: ["/tmp/qa-sidebar"],
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
    { theme, colorStyle },
  );
  await page.goto(
    `/iframe.html?id=app-onboarding-runtime--strict-native-transport&viewMode=story&globals=theme:${theme}`,
  );
  await expect(page.locator(".sidebar")).toBeVisible({ timeout: 30000 });
  await expect(page.locator("html")).toHaveAttribute(
    "data-color-style",
    colorStyle,
  );
  await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
  await expect
    .poll(() =>
      page
        .locator(".sidebar")
        .evaluate(
          (el) =>
            getComputedStyle(el).getPropertyValue("backdrop-filter") ||
            getComputedStyle(el).getPropertyValue("-webkit-backdrop-filter"),
        ),
    )
    .toContain("blur(");
}

for (const theme of ["light", "dark"]) {
  for (const colorStyle of ["dynamic", "unified", "colorful"]) {
    test(`${theme} ${colorStyle}: navigation keeps the sidebar glass on every frame`, async ({
      page,
    }, testInfo) => {
      await boot(page, theme, colorStyle);
      // Sample before input, at DOM mutations, and on every animation frame.
      // A settled screenshot alone misses the old two-rAF backdrop-filter:none gap.
      await page.evaluate(() => {
        const initialSidebar = document.querySelector(".sidebar");
        const frames: { filter: string; opacity: string; sameNode: boolean }[] =
          [];
        const sample = () => {
          const el = document.querySelector(".sidebar");
          const style = el && getComputedStyle(el);
          frames.push({
            filter:
              style?.getPropertyValue("backdrop-filter") ||
              style?.getPropertyValue("-webkit-backdrop-filter") ||
              "none",
            opacity: style?.opacity ?? "0",
            sameNode: el === initialSidebar,
          });
        };
        let raf = 0;
        const tick = () => {
          sample();
          raf = requestAnimationFrame(tick);
        };
        const observer = new MutationObserver(sample);
        observer.observe(document.documentElement, {
          attributes: true,
          childList: true,
          subtree: true,
        });
        tick();
        Object.assign(window, {
          stopSidebarSamples: () => {
            cancelAnimationFrame(raf);
            observer.disconnect();
            return frames;
          },
        });
      });
      const before = await page
        .locator(".app-shell")
        .evaluate((el) => getComputedStyle(el).backgroundImage);
      for (const name of ["定时任务", "智能流程", "插件", "定时任务"]) {
        const item = page
          .locator(".sidebar-feature-tabs")
          .getByRole("button", { name, exact: true });
        await item.click();
        await expect(item).toHaveAttribute("aria-current", "page");
        await page.evaluate(
          () =>
            new Promise<void>((resolve) => {
              requestAnimationFrame(() =>
                requestAnimationFrame(() =>
                  requestAnimationFrame(() => resolve()),
                ),
              );
            }),
        );
      }
      if (colorStyle === "dynamic") {
        const after = await page
          .locator(".app-shell")
          .evaluate((el) => getComputedStyle(el).backgroundImage);
        expect(after).not.toBe(before);
      }
      const frames = await page.evaluate(() =>
        (
          window as unknown as {
            stopSidebarSamples: () => {
              filter: string;
              opacity: string;
              sameNode: boolean;
            }[];
          }
        ).stopSidebarSamples(),
      );
      expect(frames.length).toBeGreaterThan(5);
      expect(
        frames.filter(
          (frame) =>
            !frame.filter.includes("blur(") ||
            frame.opacity !== "1" ||
            !frame.sameNode,
        ),
      ).toEqual([]);
      await page.screenshot({
        path: testInfo.outputPath("sidebar-material.png"),
      });
    });
  }
}
