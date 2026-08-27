import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const app = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");
const projectStyles = await readFile(
  new URL("../../styles/features/shell/layout/projects.css", import.meta.url),
  "utf8",
);
const sessionStyles = await readFile(
  new URL("../../styles/features/shell/layout/sessions.css", import.meta.url),
  "utf8",
);
const sessionList = await readFile(
  new URL("../../components/chat/SidebarSessionList.tsx", import.meta.url),
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
  assert.ok(workspaceLabel > primaryActions, "workspace label should follow primary actions");
  assert.ok(featureNavigation > workspaceLabel, "workspace navigation should follow its label");
  assert.equal(
    app.match(/className="sidebar-session-search sidebar-global-search"/g)?.length,
    1,
    "session search should have one global entry point",
  );
});

test("project and recent actions stay with the section they affect", () => {
  const projects = app.indexOf('t("sidebar.projects")');
  const addProject = app.indexOf('className="sidebar-add-btn"', projects);
  const recent = app.indexOf('t("sidebar.recent")');
  const archive = app.indexOf('className={`sidebar-session-filter-btn', recent);

  assert.ok(projects >= 0 && addProject > projects, "new project action must follow the Projects heading");
  assert.ok(recent >= 0 && archive > recent, "archive action must follow the Recent heading");
  assert.match(app, /aria-expanded=\{!collapsedSections\.has\("projects"\)\}/);
  assert.match(app, /aria-expanded=\{!collapsedSections\.has\("recent"\)\}/);
  assert.match(
    app,
    /<span className="sidebar-section-title">[\s\S]*?<\/span>[\s\S]*?<ChevronRight/,
    "section chevrons should follow their labels",
  );
});

test("sidebar hierarchy stays compact and keeps a separated footer", () => {
  const primaryActions = rule(projectStyles, ".sidebar-primary-actions");
  const newChat = rule(projectStyles, ".sidebar-new-chat");
  const groupLabel = rule(projectStyles, ".sidebar-group-label");
  const activeNav = rule(projectStyles, ".sidebar-feature-tab.is-active::before");
  const sectionToggle = rule(projectStyles, ".sidebar-section-toggle");
  const interactiveSectionToggle = rule(
    projectStyles,
    ".sidebar-section-toggle:is(:hover, :focus-visible, :active)",
  );
  const sectionChevron = rule(projectStyles, ".sidebar-section-chevron");
  const sectionActions = rule(projectStyles, ".sidebar-section-actions");
  const sectionTitle = rule(projectStyles, ".sidebar-section-title");
  const activeProjectIndicator = rule(
    projectStyles,
    ".sidebar-project.is-active > .sidebar-project-header::before",
  );
  const footer = rule(projectStyles, ".sidebar-footer");
  const footerDivider = rule(projectStyles, ".sidebar-footer::before");

  assert.ok(primaryActions, "missing primary action row styles");
  assert.match(primaryActions, /display:\s*flex;/);
  assert.ok(newChat, "missing new-chat styles");
  assert.match(newChat, /min-height:\s*40px;/);
  assert.match(newChat, /font-size:\s*14px;/);
  assert.ok(groupLabel, "missing workspace group heading styles");
  assert.match(groupLabel, /font-size:\s*14px;/);
  assert.ok(activeNav, "active workspace navigation needs a position marker");
  assert.match(activeNav, /width:\s*2px;/);
  assert.ok(sectionToggle, "missing collapsible section toggle styles");
  assert.match(sectionToggle, /gap:\s*6px;/);
  assert.ok(interactiveSectionToggle, "section headings need explicit interaction styles");
  assert.match(interactiveSectionToggle, /background:\s*transparent;/);
  assert.ok(sectionChevron, "missing section chevron styles");
  assert.match(sectionChevron, /opacity:\s*0;/);
  assert.ok(sectionActions, "missing section action styles");
  assert.match(sectionActions, /opacity:\s*0;/);
  assert.match(sectionActions, /pointer-events:\s*none;/);
  assert.ok(sectionTitle, "missing section heading styles");
  assert.match(sectionTitle, /font-size:\s*14px;/);
  assert.match(sectionTitle, /font-weight:\s*650;/);
  assert.ok(activeProjectIndicator, "active project needs a position marker");
  assert.match(activeProjectIndicator, /left:\s*2px;/);
  assert.ok(footer, "missing fixed sidebar footer");
  assert.match(footer, /margin-top:\s*auto;/);
  assert.ok(footerDivider, "sidebar footer should be visually separated");
});

test("session activity uses trailing status and hover-revealed tools", () => {
  const actions = rule(sessionStyles, ".sidebar-session-actions");
  const unreadDot = rule(sessionStyles, ".session-status-unread-dot");

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
  assert.match(actions, /opacity:\s*0;/);
  assert.match(actions, /pointer-events:\s*none;/);
  assert.ok(unreadDot, "missing unread marker styles");
  assert.match(unreadDot, /border-radius:\s*50%;/);
});
