import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const cssUrl = new URL(
  "../../styles/features/shell/header.css",
  import.meta.url,
);
const unifiedColorUrl = new URL(
  "../../styles/tokens/unified-color.css",
  import.meta.url,
);
const titleUrl = new URL(
  "../../components/chat/ConversationTitle.tsx",
  import.meta.url,
);
const tipsUrl = new URL("../../hooks/ui/useBeautifyTips.ts", import.meta.url);
const appUrl = new URL("../../App.tsx", import.meta.url);
const modelPickerUrl = new URL(
  "../../components/agents/ModelPicker.tsx",
  import.meta.url,
);
const projectStylesUrl = new URL(
  "../../styles/features/shell/layout/projects.css",
  import.meta.url,
);

test("conversation title truncates and only exposes its full text when overflowing", async () => {
  const [css, source, tips] = await Promise.all([
    readFile(cssUrl, "utf8"),
    readFile(titleUrl, "utf8"),
    readFile(tipsUrl, "utf8"),
  ]);

  assert.match(css, /\.conversation-title[\s\S]*?text-overflow: ellipsis;/);
  assert.match(source, /scrollWidth > node\.clientWidth \+ 1/);
  assert.match(source, /const fullTitle = accessibleTitle \?\? title;/);
  assert.match(source, /data-tip=\{overflowing \? fullTitle : undefined\}/);
  assert.match(source, /data-tip-delay=\{overflowing \? "400" : undefined\}/);
  assert.match(source, /aria-label=\{`\$\{fullTitle\} · \$\{renameLabel\}`\}/);
  assert.match(source, /onClick=\{onRename\}/);
  assert.match(tips, /getAttribute\("data-tip-delay"\)/);
  assert.match(tips, /showFor\(el, true\)/);
});

test("header tool groups keep the titlebar surface with a compact shadow", async () => {
  const css = await readFile(cssUrl, "utf8");
  const expectedSurface = [
    "background: var(--titlebar-menu-bg);",
    "border: 1px solid var(--titlebar-menu-border);",
    "box-shadow: var(--header-chip-shadow);",
    "backdrop-filter: var(--titlebar-menu-blur);",
  ];

  for (const selector of [".chat-header-tools", ".model-picker-trigger"]) {
    const blocks = [
      ...css.matchAll(new RegExp(`\\${selector} \\{([\\s\\S]*?)\\n\\}`, "g")),
    ].map((match) => match[1]);
    assert.ok(
      blocks.some((block) =>
        expectedSurface.every((declaration) => block.includes(declaration)),
      ),
    );
  }
});

test("compact header controls share the 32px height contract", async () => {
  const css = await readFile(cssUrl, "utf8");

  for (const selector of [".chat-header-tools", ".model-picker-trigger"]) {
    const blocks = [
      ...css.matchAll(new RegExp(`\\${selector} \\{([\\s\\S]*?)\\n\\}`, "g")),
    ].map((match) => match[1]);
    assert.ok(blocks.some((block) => /height:\s*32px;/.test(block)));
  }

  assert.match(
    css,
    /\.header-icon-btn\s*\{[\s\S]*?width:\s*28px;[\s\S]*?height:\s*28px;/,
  );
});

test("model picker panel shares the titlebar session-menu surface", async () => {
  const [css, projectStyles] = await Promise.all([
    readFile(cssUrl, "utf8"),
    readFile(projectStylesUrl, "utf8"),
  ]);
  const panel = css.match(/\.model-picker-panel\s*\{(?<body>[\s\S]*?)\n\}/)
    ?.groups?.body;
  const titlebarMenu = projectStyles.match(
    /\.project-context-menu\s*\{(?<body>[\s\S]*?)\n\}/,
  )?.groups?.body;

  assert.ok(panel, "missing model picker panel styles");
  assert.ok(titlebarMenu, "missing titlebar session menu styles");
  for (const declaration of [
    "background: var(--titlebar-menu-bg);",
    "border: 1px solid var(--titlebar-menu-border);",
    "box-shadow: var(--titlebar-menu-shadow);",
    "backdrop-filter: var(--titlebar-menu-blur);",
  ]) {
    assert.ok(
      panel.includes(declaration),
      `model picker is missing ${declaration}`,
    );
    assert.ok(
      titlebarMenu.includes(declaration),
      `titlebar menu is missing ${declaration}`,
    );
  }
});

test("model picker flyout portals beyond the frosted header compositing layer", async () => {
  const source = await readFile(modelPickerUrl, "utf8");

  assert.match(source, /import \{ createPortal \} from "react-dom";/);
  assert.match(source, /mode:\s*"fixed"/);
  assert.match(source, /flyoutRef\.current\?\.contains\(target\)/);
  assert.match(source, /createPortal\([\s\S]*?document\.body/);
});

test("active header tools remain distinct from hover in light and dark themes", async () => {
  const css = await readFile(cssUrl, "utf8");
  const active = css.match(
    /\.header-icon-btn\.is-active\s*\{(?<body>[\s\S]*?)\n\}/,
  )?.groups?.body;
  const darkActive = css.match(
    /html\[data-theme="dark"\] \.header-icon-btn\.is-active\s*\{(?<body>[\s\S]*?)\n\}/,
  )?.groups?.body;

  assert.ok(active, "missing active header tool styles");
  assert.match(active, /24%/);
  assert.match(active, /inset 0 0 0 1px[\s\S]*?42%/);
  assert.match(active, /0 2px 7px[\s\S]*?18%/);
  assert.ok(darkActive, "missing dark active header tool styles");
  assert.match(darkActive, /28%/);
  assert.match(darkActive, /inset 0 0 0 1px[\s\S]*?48%/);
});

test("unified color modes keep the conversation title neutral", async () => {
  const css = await readFile(unifiedColorUrl, "utf8");

  assert.match(
    css,
    /\.content-header--chat[\s\S]*?\.conversation-title\[data-tone\][\s\S]*?\{[\s\S]*?color:\s*var\(--ink\);/,
  );
});

test("conversation header follows the active project icon", async () => {
  const source = await readFile(appUrl, "utf8");

  assert.match(
    source,
    /<ProjectFolderIcon\s+[\s\S]*?iconId=\{activeProject\?\.icon\}[\s\S]*?expanded=\{false\}[\s\S]*?size=\{18\}/,
  );
});
