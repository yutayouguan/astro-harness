import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

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
const appSource = await readFile(
  new URL("../../App.tsx", import.meta.url),
  "utf8",
);
const messagesSource = await readFile(
  new URL("../../i18n/messages.ts", import.meta.url),
  "utf8",
);

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

test("settings navigation uses one Astro icon family and one optical canvas", () => {
  assert.match(settingsTabsSource, /from "\.\.\/\.\.\/components\/icons"/);
  assert.doesNotMatch(settingsTabsSource, /from "lucide-react"/);
  assert.match(settingsTabsSource, /labelKey:\s*MessageKey/);
  assert.doesNotMatch(settingsTabsSource, /label:\s*"[^"\n]*[\u3400-\u9fff]/);
  assert.match(settingsTabsSource, /IconContext/);
  assert.match(settingsTabsSource, /IconDiagnostics/);

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
  assert.match(
    messagesSource,
    /"settings\.sidebar\.tab\.general": "偏好设置"/,
  );
  assert.match(
    messagesSource,
    /"settings\.sidebar\.tab\.providers": "模型服务"/,
  );
  assert.match(
    messagesSource,
    /"settings\.sidebar\.tab\.tools": "工具与技能"/,
  );
  assert.match(
    messagesSource,
    /"settings\.sidebar\.tab\.about": "关于 Astro"/,
  );
  assert.match(messagesSource, /"settings\.sidebar\.group\.basics": "General"/);
  assert.match(
    messagesSource,
    /"settings\.sidebar\.group\.intelligence": "Agents"/,
  );
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
});
