import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { readI18nCatalogs } from "./i18nCatalogSource.mjs";

const projectStyles = await readFile(
  new URL("../../styles/features/shell/layout/projects.css", import.meta.url),
  "utf8",
);
const preferenceStyles = await readFile(
  new URL("../../styles/features/preferences.css", import.meta.url),
  "utf8",
);
const a11yStyles = await readFile(
  new URL("../../styles/tokens/a11y.css", import.meta.url),
  "utf8",
);
const settingsTabsSource = await readFile(
  new URL("settingsTabs.ts", import.meta.url),
  "utf8",
);
const providerStyles = await readFile(
  new URL("../../styles/features/providers.css", import.meta.url),
  "utf8",
);
const toolStyles = await readFile(
  new URL("../../styles/features/tools.css", import.meta.url),
  "utf8",
);
const marketStyles = await readFile(
  new URL("../../styles/features/model-market.css", import.meta.url),
  "utf8",
);
const insightStyles = await readFile(
  new URL("../../styles/features/insights.css", import.meta.url),
  "utf8",
);
const memoryStyles = await readFile(
  new URL("../../styles/features/memory.css", import.meta.url),
  "utf8",
);
const appSource = await readFile(
  new URL("../../App.tsx", import.meta.url),
  "utf8",
);
const messagesSource = await readI18nCatalogs();

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(new RegExp(`${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`))
    ?.groups?.body;
}

test("active sidebar settings navigation uses a flat tinted selection", () => {
  const sidebar = rule(
    projectStyles,
    ".sidebar-settings-nav .settings-sidebar-item.is-active",
  );

  assert.ok(sidebar, "missing active settings navigation rule");
  assert.match(sidebar, /background:\s*var\(--tone-soft/);
  assert.match(sidebar, /border-color:\s*color-mix\([\s\S]*?var\(--tone/);
  assert.match(sidebar, /box-shadow:\s*none;/);
  assert.match(sidebar, /backdrop-filter:\s*none;/);
  assert.doesNotMatch(sidebar, /var\(--glass-rim\)/);
});

test("settings navigation hover and press states give restrained feedback", () => {
  const hover = rule(
    projectStyles,
    ".sidebar-settings-nav .settings-sidebar-item:hover",
  );
  const pressed = rule(
    projectStyles,
    ".sidebar-settings-nav .settings-sidebar-item:active",
  );

  assert.ok(hover, "missing settings navigation hover rule");
  assert.match(hover, /border-color:\s*transparent;/);
  assert.match(hover, /background:\s*color-mix\([\s\S]*?var\(--ink\) 4%/);
  assert.match(hover, /box-shadow:\s*none;/);
  assert.ok(pressed, "missing settings navigation pressed rule");
  assert.match(pressed, /transform:\s*scale\(0\.985\);/);
});

test("settings sidebar uses grouped compact navigation and a quiet search field", () => {
  assert.match(projectStyles, /\.sidebar-settings-search\s*\{/);
  assert.match(projectStyles, /\.sidebar-settings-group-label\s*\{/);

  const search = rule(projectStyles, ".sidebar-settings-search");
  const focusedSearch = rule(
    projectStyles,
    ".sidebar-settings-search:focus-within",
  );
  assert.ok(search, "missing settings search rule");
  assert.ok(focusedSearch, "missing focused settings search rule");
  assert.match(search, /background:\s*color-mix\([\s\S]*?var\(--ink\) 4%/);
  assert.match(
    focusedSearch,
    /background:\s*color-mix\([\s\S]*?var\(--ink\) 5%/,
  );
  assert.doesNotMatch(search, /var\(--bg-base\)/);
  assert.doesNotMatch(focusedSearch, /var\(--bg-base\)/);

  const item = rule(
    projectStyles,
    ".sidebar-settings-nav .settings-sidebar-item",
  );
  assert.ok(item, "missing settings navigation item rule");
  assert.match(item, /min-height:\s*36px;/);
  assert.match(item, /border-radius:\s*10px;/);
});

test("settings content panels consume one shared glass material contract", () => {
  const content = rule(projectStyles, ".settings-content-inline");
  assert.ok(content, "missing settings content material scope");
  assert.match(
    content,
    /--settings-panel-border:\s*var\(--content-card-border\);/,
  );
  assert.match(
    content,
    /--settings-panel-background:\s*var\(--content-card-background\);/,
  );
  assert.match(
    content,
    /--settings-panel-shadow:\s*var\(--content-card-shadow\);/,
  );
  assert.match(
    content,
    /--settings-panel-backdrop:\s*var\(--content-card-backdrop\);/,
  );

  for (const [name, source] of [
    ["preferences", preferenceStyles],
    ["providers", providerStyles],
    ["tools", toolStyles],
    ["model market", marketStyles],
    ["insights", insightStyles],
    ["memory", memoryStyles],
  ]) {
    assert.match(
      source,
      /var\(\s*--settings-panel-background/,
      `${name} must use the shared settings background`,
    );
    assert.match(
      source,
      /var\(\s*--settings-panel-border/,
      `${name} must use the shared settings border`,
    );
    assert.match(
      source,
      /var\(\s*--settings-panel-backdrop/,
      `${name} must use the shared settings backdrop`,
    );
  }
});

test("embedded settings clip tinted sections to the shared panel radius", () => {
  const clip = rule(
    projectStyles,
    ".settings-content-inline > .prefs-page.is-embedded .prefs-category-stack",
  );

  assert.ok(clip, "missing embedded settings clipping rule");
  assert.match(clip, /overflow:\s*hidden;/);
});

test("preference-backed settings tabs render their glass material on the first frame", () => {
  const stack = rule(preferenceStyles, ".prefs-category-stack");

  assert.ok(stack, "missing preference category stack rule");
  assert.doesNotMatch(stack, /animation\s*:/);
  assert.doesNotMatch(preferenceStyles, /@keyframes\s+prefs-category-enter/);
});

test("settings navigation uses one Astro icon family and one optical canvas", () => {
  assert.match(settingsTabsSource, /from "\.\.\/\.\.\/components\/icons"/);
  assert.doesNotMatch(settingsTabsSource, /from "lucide-react"/);
  assert.match(settingsTabsSource, /labelKey:\s*MessageKey/);
  assert.doesNotMatch(settingsTabsSource, /label:\s*"[^"\n]*[\u3400-\u9fff]/);
  assert.match(settingsTabsSource, /IconContext/);
  assert.match(settingsTabsSource, /IconDiagnostics/);
  assert.match(
    settingsTabsSource,
    /id:\s*"preferences:appearance",[\s\S]*?Icon:\s*IconAppearance/,
  );
  assert.match(
    settingsTabsSource,
    /id:\s*"preferences:about",[\s\S]*?Icon:\s*IconAbout/,
  );

  const icon = rule(projectStyles, ".settings-sidebar-icon");
  const iconSvg = rule(projectStyles, ".settings-sidebar-icon svg");
  assert.ok(icon, "missing settings icon canvas rule");
  assert.ok(iconSvg, "missing settings icon svg rule");
  assert.match(icon, /width:\s*20px;/);
  assert.match(icon, /flex:\s*0 0 20px;/);
  assert.match(iconSvg, /width:\s*18px;/);
  assert.match(iconSvg, /stroke-width:\s*1\.7;/);
});

test("settings sidebar labels and search follow the active locale", () => {
  assert.match(appSource, /const label = t\(group\.labelKey\);/);
  assert.match(appSource, /label: t\(item\.labelKey\)/);
  assert.match(appSource, /const settingsTitle = t\(settingsTitleKey\);/);
  assert.match(
    appSource,
    /placeholder=\{t\("settings\.sidebar\.searchPlaceholder"\)\}/,
  );
  assert.match(appSource, /\[settingsQuery, t\]/);
});

test("settings sidebar uses clear category and capability names", () => {
  assert.match(messagesSource, /"settings\.sidebar\.group\.basics": "通用"/);
  assert.match(
    messagesSource,
    /"settings\.sidebar\.group\.intelligence": "智能体"/,
  );
  assert.match(messagesSource, /"settings\.sidebar\.tab\.general": "基础设置"/);
  assert.match(
    messagesSource,
    /"settings\.sidebar\.tab\.providers": "模型服务"/,
  );
  assert.match(messagesSource, /"settings\.sidebar\.tab\.context": "自动压缩"/);
  assert.match(messagesSource, /"settings\.sidebar\.tab\.tools": "工具"/);
  assert.match(messagesSource, /"settings\.sidebar\.tab\.about": "关于 Astro"/);
  assert.match(messagesSource, /"settings\.sidebar\.group\.basics": "General"/);
  assert.match(
    messagesSource,
    /"settings\.sidebar\.group\.intelligence": "Agents"/,
  );
  assert.match(
    messagesSource,
    /"settings\.sidebar\.tab\.context": "Automatic Compression"/,
  );
  assert.match(messagesSource, /"settings\.sidebar\.tab\.tools": "Tools"/);
});

test("preference category navigation keeps the shared glass material", () => {
  const categories = rule(
    preferenceStyles,
    ".prefs-category-nav-item.is-active",
  );

  assert.ok(categories, "missing active preference category navigation rule");
  assert.match(categories, /background:\s*var\(--glass-fill-soft\);/);
  assert.match(categories, /border-color:\s*color-mix\(/);
  assert.match(categories, /box-shadow:[\s\S]*var\(--glass-rim\)/);
  assert.match(categories, /backdrop-filter:\s*var\(--backdrop-glass\);/);
  assert.match(
    categories,
    /-webkit-backdrop-filter:\s*var\(--backdrop-glass\);/,
  );
});

test("reduced transparency replaces settings glass with a solid surface", () => {
  const media = a11yStyles.match(
    /@media \(prefers-reduced-transparency: reduce\)\s*\{(?<body>[\s\S]*?)\n\}/,
  )?.groups?.body;

  assert.ok(media, "missing reduced-transparency rules");
  assert.match(media, /\.settings-sidebar-item\.is-active/);
  assert.match(media, /\.prefs-category-nav-item\.is-active/);
  assert.match(media, /background:\s*var\(--glass-panel\);/);
  assert.match(
    media,
    /html\[data-theme\]\s*\{[\s\S]*?--content-card-background:\s*var\(--surface-panel-background\);[\s\S]*?--content-card-backdrop:\s*none;/,
  );
});
