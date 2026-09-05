import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const headerStyles = await readFile(
  new URL("../../styles/features/shell/header.css", import.meta.url),
  "utf8",
);
const chatStyles = await readFile(
  new URL("../../styles/features/chat/markdown.css", import.meta.url),
  "utf8",
);
const coreStyles = await readFile(
  new URL("../../styles/features/chat/core.css", import.meta.url),
  "utf8",
);
const rightPanelStyles = await readFile(
  new URL("../../styles/features/chat/right-panel.css", import.meta.url),
  "utf8",
);
const projectFilesStyles = await readFile(
  new URL("../../styles/features/chat/project-files.css", import.meta.url),
  "utf8",
);
const projectFilesPanel = await readFile(
  new URL("../../components/chat/ProjectFilesPanel.tsx", import.meta.url),
  "utf8",
);
const projectFileEditor = await readFile(
  new URL("../../components/chat/ProjectFileEditor.tsx", import.meta.url),
  "utf8",
);
const sideChatStyles = await readFile(
  new URL("../../styles/features/chat/side-chat.css", import.meta.url),
  "utf8",
);
const reviewPanelStyles = await readFile(
  new URL("../../styles/features/chat/review-panel.css", import.meta.url),
  "utf8",
);
const cronRunStyles = await readFile(
  new URL("../../styles/features/cron/run-drawer.css", import.meta.url),
  "utf8",
);
const chatView = await readFile(
  new URL("../../components/chat/ChatView.tsx", import.meta.url),
  "utf8",
);
const messages = await readFile(
  new URL("../../i18n/messages.ts", import.meta.url),
  "utf8",
);
const app = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("chat header chrome stays anchored while project files animate", () => {
  const header = rule(headerStyles, ".content-header--chat");
  const actions = rule(headerStyles, ".content-header--chat .header-actions");
  const projectFileActions = rule(
    headerStyles,
    ".content-header--chat.has-project-files .header-actions",
  );

  assert.ok(header, "missing chat header rule");
  assert.match(header, /position:\s*absolute;/);
  assert.match(header, /z-index:\s*var\(--z-titlebar\);/);
  assert.match(header, /top:\s*0;/);
  assert.match(header, /right:\s*0;/);
  assert.match(header, /left:\s*0;/);
  assert.match(header, /border-radius:\s*0;/);
  assert.match(header, /min-height:\s*var\(--titlebar-control-row-h\);/);
  assert.match(header, /padding:\s*0 16px;/);
  assert.ok(actions, "missing chat header action rule");
  assert.match(actions, /position:\s*absolute;/);
  assert.match(actions, /top:\s*50%;/);
  assert.match(actions, /right:\s*12px;/);
  assert.match(actions, /transform:\s*translateY\(-50%\);/);
  assert.doesNotMatch(actions, /transition:\s*right/);
  assert.equal(projectFileActions, undefined);
});

test("chat header is invisible at rest and adds glass only above scrolling content", () => {
  const header = rule(headerStyles, ".content-header--chat");
  const fileHeader = rule(
    headerStyles,
    ".content-header--chat.has-project-file",
  );
  const underlayLayer = rule(headerStyles, ".content-header--chat::before");
  const visibleUnderlayLayer = rule(
    headerStyles,
    ".content-header--chat.has-content-underlay::before",
  );
  const chatPane = rule(headerStyles, ".content-pane--chat");

  assert.ok(header, "missing chat header rule");
  assert.ok(underlayLayer, "missing chat header underlay layer");
  assert.ok(visibleUnderlayLayer, "missing visible header underlay rule");
  assert.ok(chatPane, "missing chat content pane rule");
  assert.match(chatPane, /position:\s*relative;/);
  assert.match(
    headerStyles,
    /> \.chat-layout-with-right\s*\{\s*--chat-header-overlay-height:\s*50px;/,
  );
  assert.match(header, /--chat-header-surface-base:\s*var\(--sidebar-bg\);/);
  assert.match(
    header,
    /--chat-header-surface-sheen:\s*var\(--sidebar-chrome-sheen, none\);/,
  );
  assert.match(
    header,
    /--chat-header-divider:\s*color-mix\(in srgb, var\(--ink\) 8%, transparent\);/,
  );
  assert.match(
    header,
    /--chat-header-filter:\s*var\(--sidebar-chrome-filter\);/,
  );
  assert.match(header, /background:\s*transparent;/);
  assert.match(header, /border:\s*0;/);
  assert.doesNotMatch(header, /border-top:/);
  assert.doesNotMatch(header, /border-left:/);
  assert.doesNotMatch(header, /border-right:/);
  assert.match(header, /box-shadow:\s*none;/);
  assert.match(
    underlayLayer,
    /background:[\s\S]*var\(--chat-header-surface-sheen\)[\s\S]*var\(--chat-header-surface-base\);/,
  );
  assert.match(
    underlayLayer,
    /border-bottom:\s*0\.5px solid var\(--chat-header-divider\);/,
  );
  assert.match(
    underlayLayer,
    /backdrop-filter:\s*var\(--chat-header-filter\);/,
  );
  assert.match(underlayLayer, /opacity:\s*0;/);
  assert.match(underlayLayer, /transition:\s*opacity 160ms ease;/);
  assert.match(visibleUnderlayLayer, /opacity:\s*1;/);
  assert.doesNotMatch(headerStyles, /var\(--bg1\) 72%/);
  assert.ok(fileHeader, "missing project file header material rule");
  assert.match(fileHeader, /--chat-header-divider:\s*transparent;/);
  assert.match(fileHeader, /background:\s*var\(--sidebar-chrome-background\);/);
  assert.match(fileHeader, /box-shadow:\s*none;/);
  assert.match(
    fileHeader,
    /backdrop-filter:\s*var\(--sidebar-chrome-filter\);/,
  );
  assert.match(header, /pointer-events:\s*none;/);
  assert.match(
    headerStyles,
    /\.content-header--chat \.page-title-block,[\s\S]*?\.content-header--chat \.conversation-menu\s*\{[\s\S]*?pointer-events:\s*auto;/,
  );
  assert.match(
    rule(headerStyles, ".content-header--chat .header-actions"),
    /pointer-events:\s*auto;/,
  );
  assert.match(app, /content-pane--chat/);
  assert.match(app, /chat\.emptyMode \? " is-welcome"/);
  assert.match(
    app,
    /chatHeaderHasUnderlay \|\| projectFiles\.tabs\.length > 0\s*\? " has-content-underlay"/,
  );
  assert.match(app, /projectFiles\.tabs\.length > 0 \? " has-project-file"/);
  assert.match(app, /onHeaderUnderlayChange=\{setChatHeaderHasUnderlay\}/);
  assert.match(
    chatView,
    /headerUnderlayRef\.current\s*\? list\.scrollTop > HEADER_UNDERLAY_EXIT_SCROLL_TOP\s*:\s*list\.scrollTop > HEADER_UNDERLAY_ENTER_SCROLL_TOP/,
  );
  assert.match(chatView, /HEADER_UNDERLAY_ENTER_SCROLL_TOP = 12;/);
  assert.match(chatView, /HEADER_UNDERLAY_EXIT_SCROLL_TOP = 2;/);
  assert.match(
    coreStyles,
    /\.content-pane--chat[\s\S]*?> \.message-list\s*\{[\s\S]*?padding-top:\s*var\(--chat-header-overlay-height, 50px\);[\s\S]*?scroll-padding-block-start:/,
  );
  assert.match(headerStyles, /@media \(prefers-reduced-transparency: reduce\)/);
  assert.match(headerStyles, /@media \(prefers-contrast: more\)/);
  assert.match(
    cronRunStyles,
    /top:\s*calc\(var\(--chat-header-overlay-height, 0px\) \+ 12px\);/,
  );
  assert.match(
    sideChatStyles,
    /margin:\s*calc\(var\(--chat-header-overlay-height, 0px\) \+ 8px\) 12px 0;/,
  );
});

test("conversation title keeps a compact optical type scale", () => {
  const title = rule(headerStyles, ".content-header--chat .conversation-title");

  assert.ok(title, "missing conversation title rule");
  assert.match(title, /font-size:\s*14px;/);
  assert.match(title, /font-weight:\s*600;/);
  assert.match(title, /letter-spacing:\s*0\.01em;/);
});

test("file workspace shares the titlebar material and keeps square edges", () => {
  const layout = rule(
    rightPanelStyles,
    ".chat-layout-with-right.has-right-dock",
  );
  const browserLayout = rule(
    rightPanelStyles,
    ".app-shell .chat-layout-with-right.has-browser",
  );
  const layoutBase = rule(rightPanelStyles, ".chat-layout-with-right");
  const runtimePanel = rule(rightPanelStyles, ".chat-right-panel");
  const projectPanel = rule(projectFilesStyles, ".project-files-panel");
  const openProjectPanel = rule(
    projectFilesStyles,
    ".project-files-panel.is-open",
  );
  const projectWorkbench = rule(projectFilesStyles, ".project-file-workbench");
  const fileChatPane = rule(
    projectFilesStyles,
    "html .chat-pane.has-project-file",
  );
  const editorBody = rule(projectFilesStyles, ".project-file-editor-body");
  const titlebarTabs = rule(projectFilesStyles, ".project-file-tabs");
  const sidePanel = rule(sideChatStyles, ".side-chat-panel");
  const reviewPanel = rule(reviewPanelStyles, ".chat-review-panel");
  const filePageBody = rule(
    projectFilesStyles,
    ".content-pane--chat > .page-body--chat.has-project-file",
  );

  assert.ok(layout, "missing docked chat layout rule");
  assert.match(layout, /border-radius:\s*28px 0 0 0;/);
  assert.ok(browserLayout, "missing browser-specific chat layout rule");
  assert.match(browserLayout, /border-top-left-radius:\s*0;/);
  assert.ok(layoutBase, "missing shared chat dock surface tokens");
  assert.match(layoutBase, /--chat-dock-inset:\s*0px;/);
  assert.match(layoutBase, /--chat-dock-radius:\s*0px;/);
  assert.match(layoutBase, /--chat-workbench-radius:\s*18px;/);
  assert.match(layoutBase, /--chat-dock-surface-border:\s*0\.5px solid/);
  assert.match(layoutBase, /--chat-dock-surface-background:/);
  assert.match(layoutBase, /--chat-dock-surface-shadow:/);
  assert.match(layoutBase, /--chat-dock-surface-filter:/);
  assert.ok(runtimePanel, "missing runtime panel rule");
  assert.match(runtimePanel, /right:\s*var\(--chat-dock-inset\);/);
  assert.match(runtimePanel, /bottom:\s*var\(--chat-dock-inset\);/);
  assert.match(
    runtimePanel,
    /backdrop-filter:\s*var\(--chat-dock-surface-filter\);/,
  );
  assert.match(
    runtimePanel,
    /-webkit-backdrop-filter:\s*var\(--chat-dock-surface-filter\);/,
  );
  assert.ok(projectPanel, "missing project files panel rule");
  assert.ok(openProjectPanel, "missing open project files panel rule");
  assert.ok(projectWorkbench, "missing project file workbench rule");
  assert.ok(fileChatPane, "missing square file chat surface rule");
  assert.ok(editorBody, "missing project file editor body rule");
  assert.ok(titlebarTabs, "missing project file titlebar tabs rule");
  assert.ok(sidePanel, "missing side chat panel rule");
  assert.ok(reviewPanel, "missing review panel rule");
  assert.ok(filePageBody, "missing edge-aligned file workspace rule");
  assert.match(filePageBody, /padding-left:\s*0;/);

  for (const panel of [runtimePanel, projectPanel, sidePanel, reviewPanel]) {
    assert.match(panel, /border:\s*var\(--chat-dock-surface-border\);/);
    assert.match(panel, /border-radius:\s*var\(--chat-dock-radius\);/);
    assert.match(panel, /background:\s*var\(--chat-dock-surface-background\);/);
    assert.match(panel, /box-shadow:\s*var\(--chat-dock-surface-shadow\);/);
    assert.match(
      panel,
      /padding-top:\s*var\(--chat-header-overlay-height, 50px\);/,
    );
  }
  for (const panel of [runtimePanel, projectPanel, sidePanel, reviewPanel]) {
    assert.match(
      panel,
      /backdrop-filter:\s*var\(--chat-dock-surface-filter\);/,
    );
  }
  assert.match(
    projectWorkbench,
    /margin:\s*var\(--chat-header-overlay-height, 50px\) 0 0;/,
  );
  assert.match(projectWorkbench, /border-radius:\s*0;/);
  assert.match(projectWorkbench, /background:\s*transparent;/);
  assert.match(projectWorkbench, /box-shadow:\s*none;/);
  assert.match(fileChatPane, /border-radius:\s*0;/);
  assert.match(
    fileChatPane,
    /background:\s*var\(--sidebar-chrome-background\);/,
  );
  assert.match(
    fileChatPane,
    /backdrop-filter:\s*var\(--sidebar-chrome-filter\);/,
  );
  assert.match(editorBody, /background:\s*transparent;/);
  assert.match(titlebarTabs, /background:\s*transparent;/);
  assert.match(openProjectPanel, /margin:\s*var\(--chat-dock-inset\);/);
  assert.match(openProjectPanel, /border-width:\s*0\.5px;/);
  assert.match(sidePanel, /margin:\s*var\(--chat-dock-inset\);/);
  assert.match(
    app,
    /page-body page-body--chat.*projectFiles\.tabs\.length > 0 \? " has-project-file"/,
  );
  assert.match(
    app,
    /projectFiles\.tabs\.length > 0 \? \([\s\S]*?<ProjectFileTabs/,
  );
  assert.match(projectFileEditor, /export function ProjectFileTabs/);
  assert.doesNotMatch(
    projectFileEditor,
    /<section className="project-file-workbench"[^>]*>[\s\S]*?<div className="project-file-tabs"/,
  );
  assert.match(
    rightPanelStyles,
    /@media \(prefers-reduced-transparency: reduce\)/,
  );
  assert.match(rightPanelStyles, /@media \(prefers-contrast: more\)/);
});

test("project files dock animates layout in both directions", () => {
  const panel = rule(projectFilesStyles, ".project-files-panel");
  const openPanel = rule(projectFilesStyles, ".project-files-panel.is-open");

  assert.ok(panel, "missing collapsed project files panel rule");
  assert.match(panel, /flex:\s*0 0 0;/);
  assert.match(panel, /width:\s*0;/);
  assert.match(panel, /border-width:\s*0;/);
  assert.match(panel, /visibility:\s*hidden;/);
  assert.match(
    panel,
    /transition:[\s\S]*flex-basis 300ms[\s\S]*width 300ms[\s\S]*transform 300ms/,
  );

  assert.ok(openPanel, "missing expanded project files panel rule");
  assert.match(
    openPanel,
    /flex-basis:\s*min\(var\(--project-files-width, 264px\), 42%\);/,
  );
  assert.match(openPanel, /border-width:\s*0\.5px;/);
  assert.match(openPanel, /visibility:\s*visible;/);
  assert.match(openPanel, /pointer-events:\s*auto;/);
  assert.match(projectFilesPanel, /new ResizeObserver\(reportRenderedWidth\)/);
  assert.match(projectFilesPanel, /getBoundingClientRect\(\)\.width/);
});

test("project file mode keeps its contextual composer placeholder", () => {
  assert.match(
    chatView,
    /workspaceContent\s*\?\s*t\("chat\.placeholderFile"\)/,
  );
  assert.match(messages, /"chat\.placeholderFile":\s*"询问或编辑当前文件…"/);
});

test("side chat uses the same layout motion contract as project files", () => {
  const slot = rule(sideChatStyles, ".side-chat-dock");
  const openSlot = rule(sideChatStyles, ".side-chat-dock.is-open");
  const panel = rule(sideChatStyles, ".side-chat-dock > .side-chat-panel");

  assert.ok(slot, "missing collapsed side-chat dock slot");
  assert.match(slot, /flex:\s*0 0 0;/);
  assert.match(slot, /width:\s*0;/);
  assert.match(slot, /visibility:\s*hidden;/);
  assert.match(
    slot,
    /transition:[\s\S]*flex-basis 300ms[\s\S]*width 300ms[\s\S]*margin 300ms/,
  );

  assert.ok(openSlot, "missing expanded side-chat dock slot");
  assert.match(
    openSlot,
    /flex-basis:\s*min\(var\(--side-chat-width, 384px\), 46%\);/,
  );
  assert.match(
    openSlot,
    /width:\s*min\(var\(--side-chat-width, 384px\), 46%\);/,
  );
  assert.match(openSlot, /margin:\s*var\(--chat-dock-inset\);/);
  assert.match(openSlot, /visibility:\s*visible;/);

  assert.ok(panel, "missing side-chat surface positioning rule");
  assert.match(panel, /position:\s*absolute;/);
  assert.match(panel, /inset:\s*0;/);
  assert.match(panel, /width:\s*100%;/);
  assert.match(panel, /margin:\s*0;/);
});

test("composer floats above a full-height conversation viewport", () => {
  const pane = rule(headerStyles, ".chat-pane");
  const composer = rule(chatStyles, ".composer-shell");
  const messages = rule(coreStyles, ".message-list");

  assert.ok(pane, "missing chat pane rule");
  assert.match(pane, /position:\s*relative;/);
  assert.ok(composer, "missing composer shell rule");
  assert.match(composer, /position:\s*absolute;/);
  assert.match(composer, /bottom:\s*14px;/);
  assert.ok(messages, "missing message list rule");
  assert.match(messages, /--composer-overlay-height/);
  assert.match(messages, /scroll-padding-block-end:/);
});

test("composer overlay clearance follows the live composer height", () => {
  assert.match(chatView, /new ResizeObserver\(syncComposerOverlayHeight\)/);
  assert.match(chatView, /--composer-overlay-height/);
  assert.match(chatView, /ref=\{composerShellRef\}/);
});

test("todo and file status stay inside the composer shell", () => {
  const composerStart = /className=\{`composer-shell/.exec(chatView)?.index ?? -1;
  const progressMatch = /<TodoProgress\s+messages=\{messages\}/.exec(chatView);
  const progress = progressMatch?.index ?? -1;
  const composerSurface = chatView.indexOf("composer composer--stacked");

  assert.ok(composerStart >= 0, "missing composer shell");
  assert.ok(
    progress > composerStart,
    "task progress must render inside composer shell",
  );
  assert.ok(
    progress < composerSurface,
    "task progress must render directly above the composer surface",
  );
});

test("floating composer shares the sidebar glass material", () => {
  const composer = rule(chatStyles, ".composer");
  const focusedComposer = rule(chatStyles, ".composer:focus-within");
  const darkComposer = rule(chatStyles, 'html[data-theme="dark"] .composer');
  const darkFocusedComposer = rule(
    chatStyles,
    'html[data-theme="dark"] .composer:focus-within',
  );

  assert.ok(composer, "missing composer rule");
  assert.match(composer, /--composer-surface-base:\s*var\(--sidebar-bg\);/);
  assert.match(
    composer,
    /background:[\s\S]*var\(--composer-surface-sheen\)[\s\S]*var\(--composer-surface-base\);/,
  );
  assert.match(
    composer,
    /border:\s*(?:0\.(?:[1-9]\d*)|[1-9]\d*(?:\.\d+)?)px solid var\(--glass-edge\);/,
  );
  assert.match(
    composer,
    /backdrop-filter:\s*blur\(\s*calc\(\s*var\(--blur-glass, 20px\)/,
  );
  assert.match(
    composer,
    /box-shadow:[\s\S]*var\(--glass-rim\)[\s\S]*rgba\(var\(--shadow-ink\),\s*0\.\d+\)/,
  );
  assert.doesNotMatch(composer, /background:\s*transparent;/);

  assert.ok(focusedComposer, "missing focused composer rule");
  assert.match(focusedComposer, /0 0 0 (?:0\.(?:[1-9]\d*)|1)px color-mix\(/);
  assert.doesNotMatch(focusedComposer, /0 0 0 2px/);

  assert.ok(darkComposer, "missing dark composer rule");
  assert.match(darkComposer, /var\(--sidebar-bg\)/);
  assert.doesNotMatch(darkComposer, /background:\s*transparent;/);

  assert.ok(darkFocusedComposer, "missing dark focused composer rule");
  assert.match(
    darkFocusedComposer,
    /0 0 0 (?:0\.(?:[1-9]\d*)|1)px color-mix\(/,
  );
  assert.doesNotMatch(darkFocusedComposer, /0 0 0 2px/);
});
