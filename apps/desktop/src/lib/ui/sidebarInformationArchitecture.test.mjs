import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const app = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");
const projectStyles = await readFile(
  new URL("../../styles/features/shell/layout/projects.css", import.meta.url),
  "utf8",
);
const tooltipStyles = await readFile(
  new URL("../../styles/components/tooltip.css", import.meta.url),
  "utf8",
);
const sessionStyles = await readFile(
  new URL("../../styles/features/shell/layout/sessions.css", import.meta.url),
  "utf8",
);
const sidebarPolishStyles = await readFile(
  new URL(
    "../../styles/features/shell/layout/sidebar-polish.css",
    import.meta.url,
  ),
  "utf8",
);
const unifiedColorStyles = await readFile(
  new URL("../../styles/tokens/unified-color.css", import.meta.url),
  "utf8",
);
const a11yStyles = await readFile(
  new URL("../../styles/tokens/a11y.css", import.meta.url),
  "utf8",
);
const sessionList = await readFile(
  new URL("../../components/chat/SidebarSessionList.tsx", import.meta.url),
  "utf8",
);
const sessionActionsMenu = await readFile(
  new URL("../../components/chat/SessionActionsMenu.tsx", import.meta.url),
  "utf8",
);
const statusIcon = await readFile(
  new URL("../../components/chat/SessionStatusIcon.tsx", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("sidebar keeps primary actions above the workspace navigation", () => {
  const primaryActions = app.indexOf('className="sidebar-primary-actions"');
  const workspaceLabel = app.indexOf('t("sidebar.workspace")');
  const featureNavigation = app.indexOf('className="sidebar-feature-tabs"');

  assert.ok(primaryActions >= 0, "missing grouped primary actions");
  assert.ok(
    workspaceLabel > primaryActions,
    "workspace label should follow primary actions",
  );
  assert.ok(
    featureNavigation > workspaceLabel,
    "workspace navigation should follow its label",
  );
  assert.equal(
    app.match(/className="sidebar-session-search sidebar-global-search"/g)
      ?.length,
    1,
    "session search should have one global entry point",
  );
});

test("cron, loop, and plugin pages share the same top inset", () => {
  assert.match(
    app,
    /featureNav \? \([\s\S]*?<div className="page-body page-body--bare">[\s\S]*?<CronPanel[\s\S]*?<LoopPanel[\s\S]*?<PluginsPage/,
    "all three feature pages should use the shared bare-page inset",
  );
});

test("project, automation, and recent sections keep distinct responsibilities", () => {
  const projects = app.indexOf('t("sidebar.projects")');
  const addProject = app.indexOf('className="sidebar-add-btn"', projects);
  const automation = app.indexOf('t("sidebar.automationRuns")');
  const recent = app.indexOf('t("sidebar.recent")');
  const archive = app.indexOf("className={`sidebar-session-filter-btn", recent);

  assert.ok(
    projects >= 0 && addProject > projects,
    "new project action must follow the Projects heading",
  );
  assert.ok(
    automation > projects && recent > automation,
    "automation runs should sit between projects and recent sessions",
  );
  assert.ok(
    recent >= 0 && archive > recent,
    "archive action must follow the Recent heading",
  );
  assert.match(app, /aria-expanded=\{!collapsedSections\.has\("projects"\)\}/);
  assert.match(
    app,
    /aria-expanded=\{!collapsedSections\.has\("automation"\)\}/,
  );
  assert.match(app, /aria-expanded=\{!collapsedSections\.has\("recent"\)\}/);
  assert.match(app, /placement="pinned"/);
  assert.match(app, /placement="project"/);
  assert.match(app, /placement="automation"/);
  assert.match(app, /placement="recent"/);
  assert.match(
    app,
    /<span className="sidebar-section-title">[\s\S]*?<\/span>[\s\S]*?<ChevronRight/,
    "section chevrons should follow their labels",
  );
});

test("project folders only expand while sessions own selection", () => {
  assert.match(app, /className="sidebar-project"/);
  assert.doesNotMatch(app, /selectedSidebarProjectId/);
  assert.doesNotMatch(
    app,
    /className=\{`sidebar-project[^`]*is-active/,
    "project folders should never render a selected class",
  );
  assert.match(app, /aria-expanded=\{!collapsedProjects\.has\(proj\.id\)\}/);
  assert.match(
    app,
    /const toggleVisibleProject = useCallback\([\s\S]*?setCollapsedProjects\([\s\S]*?next\.has\(projectId\)[\s\S]*?next\.add\(projectId\);[\s\S]*?return next;/,
    "project folder clicks should only toggle expansion",
  );
  assert.match(
    app,
    /className="sidebar-project-name"[\s\S]*?onClick=\{\(\) => toggleVisibleProject\(proj\.id\)\}/,
  );
  assert.match(sessionList, /aria-current=\{isActive \? "page" : undefined\}/);
  assert.doesNotMatch(sidebarPolishStyles, /\.sidebar-project\.is-active/);
  assert.doesNotMatch(projectStyles, /\.sidebar-project\.is-active/);
});

test("icon-only sidebar uses meaningful section controls instead of detached chevrons", () => {
  assert.match(app, /<Pin size=\{17\} strokeWidth=\{1\.8\} \/>/);
  assert.match(app, /<FolderTree size=\{18\} strokeWidth=\{1\.8\} \/>/);
  assert.match(app, /<Activity size=\{17\} strokeWidth=\{1\.8\} \/>/);
  assert.match(app, /<History size=\{18\} strokeWidth=\{1\.8\} \/>/);
  assert.match(
    app,
    /const sidebarContentExpanded =\s*sidebar\.showSidebarLabels \|\| sidebarRailPreview;/,
    "the rail preview should preserve the saved label preference",
  );
  assert.match(
    app,
    /if \(!sidebarContentExpanded\)[\s\S]*?setSidebarRailPreview\(true\);/,
    "collapsed section controls should preview the labelled tree",
  );
  assert.match(
    app,
    /className="sidebar-project-name"[\s\S]*?onClick=\{\(\) => toggleVisibleProject\(proj\.id\)\}/,
    "collapsed project folders should preview their expanded contents",
  );
  assert.match(
    sidebarPolishStyles,
    /\.sidebar:not\(\.is-labels\) \.sidebar-section-icon\s*\{[\s\S]*?display:\s*grid;/,
  );
  assert.match(
    sidebarPolishStyles,
    /\.sidebar:not\(\.is-labels\) \.sidebar-section-chevron\s*\{[\s\S]*?display:\s*none;/,
  );
  assert.match(
    sidebarPolishStyles,
    /\.sidebar\.is-pinned\.is-rail-preview\s*\{[\s\S]*?position:\s*absolute;/,
    "the temporary preview should leave normal flex layout",
  );
  assert.match(
    sidebarPolishStyles,
    /\.body-row:has\(> \.sidebar\.is-pinned\.is-rail-preview\)\s*\{[\s\S]*?padding-left:\s*var\(--sidebar-w\);/,
    "the temporary preview should overlay content instead of shifting it",
  );
});

test("sidebar hierarchy stays compact and keeps a separated footer", () => {
  const pinnedSidebar = rule(sidebarPolishStyles, ".sidebar.is-pinned");
  const labelledSidebar = rule(sidebarPolishStyles, ".sidebar.is-labels");
  const primaryActions = rule(sidebarPolishStyles, ".sidebar-primary-actions");
  const newChat = rule(sidebarPolishStyles, ".sidebar-new-chat");
  const groupLabel = rule(sidebarPolishStyles, ".sidebar-group-label");
  const activeNav = rule(sidebarPolishStyles, ".sidebar-feature-tab.is-active");
  const sectionToggle = rule(sidebarPolishStyles, ".sidebar-section-toggle");
  const sectionChevron = rule(sidebarPolishStyles, ".sidebar-section-chevron");
  const sectionTitle = rule(sidebarPolishStyles, ".sidebar-section-title");
  const projectGuide = rule(
    sidebarPolishStyles,
    ".sidebar-project > .sidebar-sessions::before",
  );
  const session = rule(sidebarPolishStyles, ".sidebar-session-item");
  const activeSession = rule(
    sidebarPolishStyles,
    ".sidebar-session-item.is-active",
  );
  const footer = rule(sidebarPolishStyles, ".sidebar-footer");
  const settingsButton = rule(sidebarPolishStyles, ".sidebar-settings-btn");
  const footerDivider = rule(sidebarPolishStyles, ".sidebar-footer::before");

  assert.ok(pinnedSidebar, "missing stable pinned sidebar surface");
  assert.match(
    pinnedSidebar,
    /background:\s*var\(--sidebar-chrome-background\);/,
  );
  assert.match(pinnedSidebar, /box-shadow:\s*none;/);
  assert.match(
    pinnedSidebar,
    /backdrop-filter:\s*var\(--sidebar-chrome-filter\);/,
  );
  assert.ok(labelledSidebar, "missing labelled sidebar spacing");
  assert.match(labelledSidebar, /padding-inline:\s*16px;/);
  assert.ok(primaryActions, "missing primary action row styles");
  assert.match(primaryActions, /gap:\s*4px;/);
  assert.match(primaryActions, /margin:\s*0 0 20px;/);
  assert.ok(newChat, "missing new-chat styles");
  assert.match(newChat, /min-height:\s*40px;/);
  assert.match(newChat, /font-size:\s*14px;/);
  assert.match(
    newChat,
    /box-shadow:\s*none;/,
    "new chat should sit flat in the sidebar",
  );
  assert.match(
    newChat,
    /backdrop-filter:\s*none;/,
    "new chat should not stack another glass layer",
  );
  assert.ok(groupLabel, "missing workspace group heading styles");
  assert.match(groupLabel, /margin:\s*0 4px 6px;/);
  assert.match(sidebarPolishStyles, /font-size:\s*12px;/);
  assert.ok(
    activeNav,
    "active workspace navigation needs a quiet selected state",
  );
  assert.match(
    activeNav,
    /background:\s*color-mix\(in srgb, var\(--ink\) 8%, transparent\);/,
  );
  assert.match(activeNav, /border-color:\s*transparent;/);
  assert.match(activeNav, /box-shadow:\s*none;/);
  assert.ok(sectionToggle, "missing collapsible section toggle styles");
  assert.match(sectionToggle, /gap:\s*5px;/);
  assert.ok(sectionChevron, "missing section chevron styles");
  assert.match(sectionChevron, /opacity:\s*0\.4;/);
  assert.ok(sectionTitle, "missing section heading styles");
  assert.ok(
    projectGuide,
    "project nesting should explicitly remove the long guide line",
  );
  assert.match(projectGuide, /display:\s*none;/);
  assert.ok(session, "missing session row density styles");
  assert.match(session, /min-height:\s*38px;/);
  assert.ok(activeSession, "missing active session state");
  assert.match(activeSession, /border-color:\s*transparent;/);
  assert.match(activeSession, /box-shadow:\s*none;/);
  assert.ok(footer, "missing fixed sidebar footer");
  assert.match(footer, /min-height:\s*46px;/);
  assert.match(footer, /padding:\s*6px 0 0;/);
  assert.ok(settingsButton, "missing sidebar settings button styles");
  assert.match(settingsButton, /min-height:\s*36px;/);
  assert.match(settingsButton, /font-size:\s*13\.5px;/);
  assert.match(
    rule(sidebarPolishStyles, ".sidebar-settings-btn svg"),
    /flex-basis:\s*17px;/,
  );
  assert.match(
    app,
    /<IconSettings width=\{17\} height=\{17\} strokeWidth=\{1\.8\} \/>/,
    "sidebar preferences should use the dedicated gear icon",
  );
  assert.ok(footerDivider, "sidebar footer should be visually separated");
  assert.match(footerDivider, /height:\s*1px;/);
});

test("left sidebar shares the flush glass chrome contract", () => {
  const sidebar = rule(sidebarPolishStyles, ".sidebar");
  const unifiedSidebar = rule(
    unifiedColorStyles,
    'html:is([data-color-style="unified"], [data-color-style="dynamic"]) .sidebar',
  );

  assert.ok(sidebar, "missing sidebar surface rule");
  assert.match(sidebar, /--sidebar-chrome-background:/);
  assert.match(sidebar, /border-radius:\s*0;/);
  assert.match(sidebar, /border-right:\s*0\.5px\s+solid\s+color-mix\(/);
  assert.match(sidebar, /background:\s*var\(--sidebar-chrome-background\);/);
  assert.match(sidebar, /box-shadow:\s*none;/);
  assert.match(sidebar, /backdrop-filter:\s*var\(--sidebar-chrome-filter\);/);
  assert.ok(unifiedSidebar, "missing unified sidebar sheen override");
  assert.match(unifiedSidebar, /--sidebar-chrome-sheen:\s*linear-gradient\(/);
  assert.doesNotMatch(unifiedSidebar, /\n\s*background:/);
  assert.match(a11yStyles, /--sidebar-chrome-filter:\s*none;/);
  assert.match(
    a11yStyles,
    /--sidebar-chrome-background:\s*var\(--sidebar-bg\);/,
  );
});

test("session activity preserves title space and progressively reveals tools", () => {
  const actions = rule(sessionStyles, ".sidebar-session-actions");
  const sessionMain = rule(sessionStyles, ".sidebar-session-main");
  const pinAction = rule(sessionStyles, ".sidebar-session-action-btn.is-pin");
  const unreadDot = rule(sessionStyles, ".session-status-unread-dot");
  const titleWrap = rule(sessionStyles, ".sidebar-session-title-wrap");
  const title = rule(sessionStyles, ".sidebar-session-title");
  const scrollableTitle = rule(
    sessionStyles,
    '.sidebar-session-item:hover .sidebar-session-title[data-scrollable="true"]',
  );

  assert.match(sessionList, /const showUnread = unread && status === "idle";/);
  assert.match(
    sessionList,
    /<span className="sidebar-session-time">[\s\S]*?<SessionStatusIcon/,
    "status indicator should trail the timestamp",
  );
  assert.match(statusIcon, /if \(status === "idle" && !unread\) return null;/);
  assert.match(statusIcon, /className="session-status-icon-spin"/);
  assert.match(statusIcon, /className="session-status-unread-dot"/);
  assert.ok(actions, "missing session action styles");
  assert.match(actions, /background:\s*transparent;/);
  assert.match(actions, /border:\s*0;/);
  assert.match(actions, /box-shadow:\s*none;/);
  assert.match(actions, /opacity:\s*1;/);
  assert.match(actions, /pointer-events:\s*auto;/);
  assert.doesNotMatch(actions, /backdrop-filter:/);
  assert.ok(sessionMain, "session title needs a reserved action gutter");
  assert.match(sessionMain, /padding:\s*7px 66px 7px 8px;/);
  assert.match(
    sidebarPolishStyles,
    /\.sidebar-session-main\s*\{[\s\S]*?padding:\s*8px 66px 8px 9px;/,
    "the final sidebar polish layer must keep the action gutter",
  );
  assert.ok(pinAction, "pin action needs progressive disclosure styles");
  assert.match(pinAction, /opacity:\s*0;/);
  assert.match(pinAction, /pointer-events:\s*none;/);
  assert.match(
    sessionStyles,
    /\.sidebar-session-item:is\(:hover, :focus-within\)[\s\S]*?\.sidebar-session-action-btn\.is-pin\s*\{[\s\S]*?opacity:\s*1;/,
  );
  const sessionItem = sessionList.slice(
    sessionList.indexOf("function SessionItem"),
  );
  assert.match(sessionItem, /className="sidebar-session-action-btn is-pin"/);
  assert.match(sessionItem, /className="sidebar-session-action-btn is-more"/);
  assert.doesNotMatch(sessionItem, /onArchiveToggle|ArchiveRestoreData/);
  assert.match(
    sessionActionsMenu,
    /archived \? "unarchive_session" : "archive_session"/,
  );
  assert.ok(unreadDot, "missing unread marker styles");
  assert.match(unreadDot, /border-radius:\s*50%;/);
  assert.ok(titleWrap, "missing session title clipping wrapper");
  assert.doesNotMatch(titleWrap, /mask-image:/);
  assert.ok(title, "missing session title styles");
  assert.match(title, /text-overflow:\s*ellipsis;/);
  assert.ok(scrollableTitle, "only clipped session titles should scroll");
  assert.match(scrollableTitle, /animation:\s*sidebar-title-scroll/);
  assert.match(sessionList, /if \(overflow > 1\)/);
  assert.match(sessionList, /el\.dataset\.scrollable = "true";/);
  assert.match(sessionList, /delete el\.dataset\.scrollable;/);
});

test("session action menu stays inside the app viewport", () => {
  assert.match(sessionActionsMenu, /useLayoutEffect\(\(\) => \{/);
  assert.match(
    sessionActionsMenu,
    /const size = measurePopoverSize\(element\);/,
  );
  assert.match(sessionActionsMenu, /anchorRect:\s*pointAnchor\(x, y\)/);
  assert.match(
    sessionActionsMenu,
    /const bounds = resolveClipBoundsAt\(x, y\);/,
  );
  assert.match(
    sessionActionsMenu,
    /size\.height > spaceBelow && spaceAbove > spaceBelow \? "above" : "below"/,
  );
  assert.match(sessionActionsMenu, /placement,/);
  assert.match(
    sessionActionsMenu,
    /element\.style\.maxHeight = `\$\{position\.maxHeight\}px`;/,
  );
  assert.match(
    sessionActionsMenu,
    /element\.style\.overflowY = position\.maxHeight < size\.height \? "auto" : "";/,
  );
});

test("archive filter and tips keep optical alignment with glass fallbacks", () => {
  const archiveButton = rule(projectStyles, ".sidebar-session-filter-btn");
  const archiveIcon = rule(projectStyles, ".sidebar-session-filter-btn > svg");
  const tip = rule(tooltipStyles, ".ui-tip");
  const tipArrow = rule(tooltipStyles, ".ui-tip[data-pos]::before");

  assert.ok(archiveButton, "archive filter needs an explicit centering reset");
  assert.match(archiveButton, /padding:\s*0;/);
  assert.match(archiveButton, /line-height:\s*0;/);
  assert.ok(archiveIcon, "archive icon needs deterministic centering");
  assert.match(archiveIcon, /margin:\s*auto;/);
  assert.ok(tip, "missing shared tip surface");
  assert.match(tip, /backdrop-filter:\s*blur\(18px\) saturate\(155%\);/);
  assert.match(tip, /linear-gradient\(/);
  assert.ok(tipArrow, "tip arrow should share the glass surface");
  assert.match(tipArrow, /width:\s*11px;/);
  assert.match(tipArrow, /height:\s*11px;/);
  assert.match(tipArrow, /border-radius:\s*2px;/);
  assert.match(tipArrow, /background:\s*var\(--ui-tip-bg\);/);
  assert.doesNotMatch(tipArrow, /border-style:\s*solid;/);
  assert.match(
    tooltipStyles,
    /@media \(prefers-reduced-transparency: reduce\)/,
  );
  assert.match(tooltipStyles, /@media \(prefers-contrast: more\)/);
});
