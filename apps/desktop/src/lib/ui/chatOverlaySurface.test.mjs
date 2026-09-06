import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const markdownUrl = new URL(
  "../../styles/features/chat/markdown.css",
  import.meta.url,
);
const sessionsUrl = new URL(
  "../../styles/features/shell/layout/sessions.css",
  import.meta.url,
);
const projectsUrl = new URL(
  "../../styles/features/shell/layout/projects.css",
  import.meta.url,
);
const a11yUrl = new URL("../../styles/tokens/a11y.css", import.meta.url);

function rule(css, selector) {
  return css.match(new RegExp(`\\${selector}\\s*\\{(?<body>[\\s\\S]*?)\\n\\}`))
    ?.groups?.body;
}

test("chat overlays reuse the titlebar menu glass while inline session tools stay flat", async () => {
  const [markdown, sessions, projects] = await Promise.all([
    readFile(markdownUrl, "utf8"),
    readFile(sessionsUrl, "utf8"),
    readFile(projectsUrl, "utf8"),
  ]);
  const titlebarMenu = rule(projects, ".project-context-menu");
  const surfaces = [
    ["conversation mode", rule(markdown, ".composer-mode-menu")],
    ["add menu", rule(markdown, ".composer-plus-menu")],
    ["context usage", rule(markdown, ".ctx-usage-popover")],
  ];
  const recipe = [
    "border: 1px solid var(--titlebar-menu-border);",
    "background: var(--titlebar-menu-bg);",
    "box-shadow: var(--titlebar-menu-shadow);",
    "backdrop-filter: var(--titlebar-menu-blur);",
  ];

  assert.ok(titlebarMenu, "missing titlebar session menu styles");
  for (const declaration of recipe) {
    assert.ok(
      titlebarMenu.includes(declaration),
      `titlebar menu is missing ${declaration}`,
    );
  }
  for (const [name, surface] of surfaces) {
    assert.ok(surface, `missing ${name} styles`);
    for (const declaration of recipe) {
      assert.ok(
        surface.includes(declaration),
        `${name} is missing ${declaration}`,
      );
    }
  }

  const sessionTools = rule(sessions, ".sidebar-session-actions");
  assert.ok(sessionTools, "missing session tools styles");
  assert.ok(sessionTools.includes("border: 0;"));
  assert.ok(sessionTools.includes("background: transparent;"));
  assert.ok(sessionTools.includes("box-shadow: none;"));
  assert.ok(!sessionTools.includes("backdrop-filter:"));
});

test("titlebar menu glass becomes opaque when transparency is reduced", async () => {
  const css = await readFile(a11yUrl, "utf8");

  assert.match(css, /--titlebar-menu-bg:\s*var\(--menu-glass-bg\);/);
  assert.match(css, /--titlebar-menu-blur:\s*none;/);
});
