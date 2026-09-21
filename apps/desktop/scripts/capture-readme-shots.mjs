#!/usr/bin/env node
/**
 * 从已构建的 Storybook（storybook-static）截取 README 配图。
 *
 *   cd apps/desktop
 *   npm run build-storybook
 *   node scripts/capture-readme-shots.mjs
 *
 * 输出目录：仓库根 `docs/images/`。截图来自真实组件（Storybook 只替换原生 transport），
 * 因此内容可复现且不含本机真实会话数据。
 */
import { chromium } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { createReadStream } from "node:fs";
import { copyFile, mkdir, stat, unlink } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DESKTOP = resolve(HERE, "..");
const REPO = resolve(DESKTOP, "../..");
const STATIC = join(DESKTOP, "storybook-static");
const OUT = join(REPO, "docs/images");
const STAGE = join(tmpdir(), "astro-readme-shots");
const PORT = Number(process.env.README_SHOT_PORT ?? 6411);
const BASE = `http://127.0.0.1:${PORT}`;

const MIME = {
  ".css": "text/css",
  ".html": "text/html",
  ".ico": "image/x-icon",
  ".js": "text/javascript",
  ".json": "application/json",
  ".map": "application/json",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
};

function serveStatic() {
  const server = createServer(async (req, res) => {
    const url = new URL(req.url ?? "/", BASE);
    const rel = normalize(decodeURIComponent(url.pathname)).replace(
      /^(\.\.[/\\])+/,
      "",
    );
    let file = join(STATIC, rel);
    try {
      if ((await stat(file)).isDirectory()) file = join(file, "index.html");
    } catch {
      file = join(STATIC, rel, "index.html");
    }
    try {
      await stat(file);
    } catch {
      res.writeHead(404).end("not found");
      return;
    }
    res.writeHead(200, {
      "content-type": MIME[extname(file)] ?? "application/octet-stream",
    });
    createReadStream(file).pipe(res);
  });
  return new Promise((done) =>
    server.listen(PORT, "127.0.0.1", () => done(server)),
  );
}

/** 浏览器的原生 transport 替身：只提供 App 外壳与导览需要的命令，不接触本机数据。 */
const APP_TRANSPORT = () => {
  const w = window;
  w.isTauri = true;
  localStorage.setItem("astro-locale", "zh");
  localStorage.setItem("astro-theme-mode", "dark");
  localStorage.setItem("astro.sidebarPinned", "1");
  let serial = 0;
  const callbacks = new Map();
  const listeners = new Map();
  const provider = {
    id: "openai",
    kind: "openai",
    display_name: "OpenAI",
    endpoint: "https://api.openai.com/v1",
    model: "gpt-5.6",
    enabled: true,
    has_api_key: true,
    key_source: "keyring",
    backend_id: "openai",
    supports_responses_api: true,
  };
  w.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main", windowLabel: "main" },
    },
    transformCallback: (fn) => {
      callbacks.set(++serial, fn);
      return serial;
    },
    unregisterCallback: (id) => callbacks.delete(id),
    convertFileSrc: (path) => path,
    invoke: async (cmd, args = {}) => {
      switch (cmd) {
        case "plugin:event|listen": {
          const id = ++serial;
          listeners.set(id, { event: args.event, handler: args.handler });
          return id;
        }
        case "plugin:event|unlisten":
          listeners.delete(args.eventId);
          return null;
        case "get_desktop_pet_state":
          return { revision: 1, enabled: false, petPath: "" };
        case "get_desktop_pet_visible":
          return false;
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
            workspace_dir: "/tmp/astro-workspace",
            memory_dir: "/tmp/astro-home",
            grpc_address: "",
            default_workspace_dir: "/tmp/astro-workspace",
          };
        case "get_providers_state":
          return {
            providers: [provider],
            provider_templates: [],
            active_provider_id: "openai",
          };
        case "list_projects":
          return [
            {
              id: "default",
              name: "主空间",
              roots: ["/tmp/astro-workspace"],
              position: 0,
            },
          ];
        case "get_permission_settings":
          return {
            preset: "workspace_write",
            sandboxHealth: { status: "available", backend: "seatbelt" },
          };
        case "get_app_icon":
          return { current: "blue", options: [] };
        case "get_cached_provider_models":
          return { models: [], latency_ms: 0, source: "static" };
        case "get_mcp_servers":
        case "get_agent_tools":
          return [];
        default:
          if (cmd.startsWith("get_")) throw Error("read unavailable: " + cmd);
          if (cmd.startsWith("list_")) return [];
          return null;
      }
    },
  };
};

const appShot = (name, extra = {}) => ({
  name,
  id: "app-onboarding-runtime--strict-native-transport",
  transport: APP_TRANSPORT,
  theme: "dark",
  ...extra,
});

const SHOTS = [
  appShot("app-chat-shell"),
  { name: "onboarding-provider", id: "app-first-run-onboarding--provider" },
  {
    name: "chat-turns",
    id: "chat-answer-panel--default",
    clip: { x: 280, y: 35, width: 740, height: 660 },
  },
  {
    name: "approval",
    id: "clarify-wizard--approval",
    clip: { x: 280, y: 470, width: 890, height: 420 },
  },
  { name: "providers", id: "settings-providerspanel--compact" },
  { name: "preferences", id: "preferences-panel--appearance" },
  { name: "ambience", id: "shell-desktopambience--default" },
  {
    name: "skills",
    id: "pages-plugins-skillcards--gallery",
    clip: { x: 30, y: 20, width: 1100, height: 340 },
  },
  {
    name: "mcp",
    id: "settings-mcpservercard--all-states",
    viewport: { width: 1440, height: 680 },
    clip: { x: 14, y: 0, width: 726, height: 680 },
  },
  {
    name: "cron",
    id: "pages-cronpanel--cards-with-detail-drawer",
    clip: { x: 16, y: 16, width: 720, height: 330 },
  },
  {
    name: "workflow",
    id: "pages-looppanel--default",
    store: { "astro.loop.viewMode": "detail" },
    after: async (page) =>
      page.locator(".loop-detail-sidebar-name").first().click(),
    settle: 1600,
  },
  { name: "desktop-pet", id: "settings-petscenestudio--preview-and-failure" },
  { name: "insights", id: "settings-insightspanel--overview" },
  { name: "storage", id: "settings-storagediagnostics--ready" },
  { name: "permissions", id: "tools-approvals--default" },
  {
    name: "browser",
    id: "chat-dock-layout--browser",
    clip: { x: 758, y: 0, width: 682, height: 900 },
  },
];

async function main() {
  const webp = spawnSync("cwebp", ["-version"]).status === 0;
  await mkdir(STAGE, { recursive: true });
  await mkdir(OUT, { recursive: true });
  const server = await serveStatic();
  const browser = await chromium.launch();
  const only = process.argv.slice(2);
  const shots = only.length
    ? SHOTS.filter((s) => only.includes(s.name))
    : SHOTS;
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 1,
  });
  try {
    for (const shot of shots) {
      const page = await context.newPage();
      if (shot.viewport) await page.setViewportSize(shot.viewport);
      if (shot.store) {
        await page.addInitScript((store) => {
          for (const [key, value] of Object.entries(store))
            localStorage.setItem(key, value);
        }, shot.store);
      }
      if (shot.transport) await page.addInitScript(shot.transport);
      const theme = shot.theme ?? "dark";
      const url = `${BASE}/iframe.html?id=${shot.id}&viewMode=story&globals=theme:${theme}`;
      await page.goto(url, { waitUntil: "load" });
      await page
        .locator("#storybook-root > *")
        .first()
        .waitFor({ state: "visible", timeout: 30_000 });
      if (shot.after) await shot.after(page);
      await page.waitForTimeout(shot.settle ?? 900);
      await page.screenshot({
        path: join(STAGE, `${shot.name}.png`),
        clip: shot.clip,
        fullPage: Boolean(shot.fullPage),
      });
      const staged = join(STAGE, `${shot.name}.png`);
      if (webp) {
        const result = spawnSync("cwebp", [
          "-quiet",
          "-q",
          "92",
          staged,
          "-o",
          join(OUT, `${shot.name}.webp`),
        ]);
        if (result.status !== 0)
          throw new Error(`cwebp failed for ${shot.name}`);
        await unlink(staged);
      } else {
        await copyFile(staged, join(OUT, `${shot.name}.png`));
        await unlink(staged);
      }
      const written = join(OUT, `${shot.name}.${webp ? "webp" : "png"}`);
      const size = (await stat(written)).size;
      console.log(
        `${shot.name.padEnd(26)} ${(size / 1024).toFixed(0)} KB  ${written.replace(REPO + "/", "")}`,
      );
    }
    if (!webp)
      console.log(
        "\n未检测到 cwebp，已输出 PNG。安装 webp 可自动转为更小的 WebP。",
      );
  } finally {
    await context.close();
    await browser.close();
    server.close();
  }
}

await main();
