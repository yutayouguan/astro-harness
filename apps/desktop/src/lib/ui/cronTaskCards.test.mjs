import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const panelUrl = new URL(
  "../../components/schedule/CronPanel.tsx",
  import.meta.url,
);
const cardsUrl = new URL(
  "../../styles/features/cron/cards.css",
  import.meta.url,
);
const drawerUrl = new URL(
  "../../styles/features/cron/run-drawer.css",
  import.meta.url,
);
const dialogUrl = new URL("../../styles/components/dialog.css", import.meta.url);
const overlayUrl = new URL("../../styles/components/overlay.css", import.meta.url);
const chatUrl = new URL(
  "../../components/chat/ChatView.tsx",
  import.meta.url,
);
const runDetailUrl = new URL(
  "../../components/schedule/CronRunDetailDrawer.tsx",
  import.meta.url,
);
const commandUrl = new URL(
  "../../../src-tauri/src/commands/cron.rs",
  import.meta.url,
);

const panel = await readFile(panelUrl, "utf8");
const cards = await readFile(cardsUrl, "utf8");
const drawer = await readFile(drawerUrl, "utf8");
const dialog = await readFile(dialogUrl, "utf8");
const overlay = await readFile(overlayUrl, "utf8");
const chat = await readFile(chatUrl, "utf8");
const runDetail = await readFile(runDetailUrl, "utf8");
const command = await readFile(commandUrl, "utf8");

test("scheduled task cards expose a dedicated detail affordance", () => {
  assert.match(panel, /className="cron-card-actions-cluster"/);
  assert.match(panel, /className="cron-card-detail-btn"/);
  assert.match(panel, /onClick=\{\(\) => openJobDrawer\(job\)\}/);
  assert.match(cards, /\.cron-card-detail-btn\s*\{/);
});

test("task drawer combines configuration, actions, and recent runs", () => {
  assert.match(runDetail, /className="cron-job-drawer"/);
  assert.match(runDetail, /className="cron-job-overview-card"/);
  assert.match(runDetail, /className="cron-job-drawer-actions"/);
  assert.match(runDetail, /runs\.slice\(0, 30\)/);
  assert.match(runDetail, /onClick=\{\(\) => onOpenRun\(run\)\}/);
  assert.match(runDetail, /backdropFilter: "none"/);
  assert.match(drawer, /@media \(prefers-reduced-motion: reduce\)/);
});

test("task drawer refreshes its own run history after actions", () => {
  assert.match(panel, /const loadDrawerJobRuns = useCallback/);
  assert.match(panel, /if \(drawerJobId === job\.id\)/);
  assert.match(panel, /if \(drawerJobId === run\.job_id\)/);
});

test("shared overlays no longer encode a fixed component hierarchy", () => {
  assert.doesNotMatch(dialog, /--z-dialog/);
  assert.doesNotMatch(overlay, /--z-overlay-(?:drawer|modal|popover)/);
});

test("cron sessions keep the answer and expose a separate floating task card", () => {
  assert.match(chat, /invoke<CronRunDto \| null>\("get_cron_run_by_session"/);
  assert.match(chat, /className="chat-cron-run-float"/);
  assert.match(chat, /<CronRunFloatingCard/);
  assert.match(chat, /onOpen=\{\(\) => setCronTaskOpen\(true\)\}/);
  assert.doesNotMatch(chat, /cronRun && m\.id === cronResultMessageId/);
  assert.match(chat, /<ChatMarkdown\s+content=\{m\.content\}/);
  assert.match(chat, /<CronTaskDetailDrawer/);
  assert.match(chat, /onOpenRun=\{setSelectedCronRun\}/);
  assert.match(chat, /\{selectedCronRun \? \(\s*<CronRunDetailDrawer/);
  assert.match(chat, /<CronRunDetailDrawer/);
  assert.match(runDetail, /export function CronRunFloatingCard/);
  assert.match(runDetail, /export function CronTaskDetailDrawer/);
  assert.match(runDetail, /export function CronRunDetailDrawer/);
  assert.match(runDetail, /modal=\{false\}/);
  assert.match(runDetail, /pointerEvents: "none"/);
  assert.match(runDetail, /trapFocus=\{false\}/);
  assert.match(command, /pub async fn get_cron_run_by_session/);
});
