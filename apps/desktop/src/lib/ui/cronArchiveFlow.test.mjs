import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (relative) => readFile(new URL(relative, import.meta.url), "utf8");

const panel = await read("../../components/schedule/CronPanel.tsx");
const drawer = await read("../../components/schedule/CronRunDetailDrawer.tsx");
const dialogContext = await read("../../hooks/ui/DialogContext.tsx");
const dialogStyles = await read("../../styles/components/dialog.css");
const cardsStyles = await read("../../styles/features/cron/cards.css");
const historyStyles = await read("../../styles/features/cron/history.css");
const viewsStyles = await read("../../styles/features/cron/views.css");
const command = await read(
  "../../../src-tauri/src/commands/automation/cron.rs",
);
const agentCron = await read(
  "../../../../../crates/agent-core/src/exec/cron.rs",
);
const cronStore = await read(
  "../../../../../crates/agent-cron/src/jobs/store.rs",
);
const sidebar = await read("../../components/chat/SidebarSessionList.tsx");
const sessionStyles = await read(
  "../../styles/features/shell/layout/sessions.css",
);
const zh = await read("../../i18n/catalogs/zh.ts");
const en = await read("../../i18n/catalogs/en.ts");

test("archived tasks leave the default list and get their own filter", () => {
  assert.match(panel, /const \[showArchived, setShowArchived\] = useState/);
  assert.match(panel, /Boolean\(j\.archived_at\) !== showArchived/);
  assert.match(panel, /className=\{`cron-archive-filter/);
  assert.match(panel, /cron-archive-filter-count/);
  assert.match(viewsStyles, /\.cron-archive-filter\s*\{/);
  assert.match(cardsStyles, /\.cron-card\.is-archived/);
  assert.match(cardsStyles, /\.cron-card-status\.is-archived/);
  assert.match(panel, /t\("cron\.statusArchived"\)/);
});

test("job menu and drawer expose archive and restore", () => {
  assert.match(panel, /invoke<CronJobDto>\("archive_cron_job"/);
  assert.match(panel, /setJobArchived\(job, true\)/);
  assert.match(panel, /setJobArchived\(job, false\)/);
  assert.match(panel, /t\("cron\.archive"\)/);
  assert.match(panel, /t\("cron\.restore"\)/);
  assert.match(drawer, /onToggleArchived/);
  assert.match(drawer, /job\.archived_at/);
  assert.match(command, /pub async fn archive_cron_job/);
});

test("deleting a task asks what happens to its session and run records", () => {
  assert.match(panel, /confirmDetailed\(\{/);
  assert.match(panel, /id: "archiveSession"/);
  assert.match(panel, /defaultChecked: true/);
  assert.match(panel, /id: "deleteRuns"/);
  assert.match(panel, /args: \{ id: job\.id, archiveSession, deleteRuns \}/);
  assert.match(command, /pub struct RemoveCronJobArgs/);
  assert.match(
    command,
    /#\[serde\(rename_all = "camelCase"\)\]\s*\n\s*pub struct RemoveCronJobArgs/,
  );
  assert.match(
    command,
    /#\[serde\(default = "default_true"\)\]\s*\n\s*pub archive_session/,
  );
  assert.match(command, /pub delete_runs: bool/);
  assert.match(dialogContext, /confirmDetailed/);
  assert.match(dialogContext, /selected: string\[\]/);
  assert.match(dialogStyles, /\.app-dialog-option\s*\{/);
});

test("a running task refuses delete and archive keeps definition and history", () => {
  // 运行中拦截：后端给出 running 状态，前端菜单/抽屉就地提示
  assert.match(command, /pub running: bool/);
  assert.match(command, /async fn running_job_ids\(\)/);
  assert.match(panel, /if \(archived && job\.running\)/);
  assert.match(panel, /if \(job\.running\) \{/);
  assert.match(panel, /t\("cron\.runningHintDelete"\)/);
  assert.match(panel, /t\("cron\.runningHintArchive"\)/);
  assert.match(panel, /data-blocked=\{job\.running \|\| undefined\}/);
  assert.match(panel, /cron-card-running/);
  assert.match(drawer, /cron-job-drawer-hint/);
  assert.match(dialogStyles, /\.app-dialog-option\s*\{/);
  assert.match(command, /has_running_for_job\(&job\.id\)/);
  assert.match(command, /任务正在执行/);
  assert.match(cronStore, /pub fn set_archived/);
  assert.match(cronStore, /job\.archived_at\.is_some\(\) \|\| job\.agent_id/);
});

test("archived cron sessions return to active when the task runs again", () => {
  assert.match(agentCron, /store\.unarchive_session\(session_id\)/);
});

test("orphaned run records are marked and purgeable", () => {
  assert.match(panel, /t\("cron\.orphanRun"\)/);
  assert.match(panel, /purge_orphaned_cron_runs/);
  assert.match(panel, /cron-history-purge/);
  assert.match(command, /pub async fn purge_orphaned_cron_runs/);
  assert.match(historyStyles, /\.cron-timeline-orphan\s*\{/);
  assert.match(historyStyles, /\.cron-history-purge\s*\{/);
});

test("sidebar explains cron sessions whose task is archived or deleted", () => {
  assert.match(sidebar, /resolveCronOwnerStates\(/);
  assert.match(sidebar, /"list_cron_jobs"/);
  assert.match(sidebar, /sidebar-session-cron-owner/);
  assert.match(sidebar, /t\("sessions\.cronOwnerMissing"\)/);
  assert.match(sidebar, /t\("sessions\.cronOwnerArchived"\)/);
  assert.match(sessionStyles, /\.sidebar-session-cron-owner\s*\{/);
  assert.match(sessionStyles, /\.sidebar-session-cron-owner\.is-missing\s*\{/);
});

test("archive and delete copy exists in both locales", () => {
  const keys = [
    "cron.archive",
    "cron.restore",
    "cron.statusArchived",
    "cron.archiveFilter",
    "cron.archiveFilterHint",
    "cron.remove.archiveSession",
    "cron.remove.archiveSessionHint",
    "cron.remove.deleteRuns",
    "cron.remove.deleteRunsHint",
    "cron.orphanRun",
    "cron.orphanPurge",
    "cron.orphanPurgeHint",
    "cron.orphanPurgeConfirm",
    "sessions.cronOwnerArchived",
    "sessions.cronOwnerMissing",
  ];
  for (const key of keys) {
    assert.ok(zh.includes(`"${key}":`), `zh missing ${key}`);
    assert.ok(en.includes(`"${key}":`), `en missing ${key}`);
  }
});
