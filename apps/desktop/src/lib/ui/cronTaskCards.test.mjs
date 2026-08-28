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

const panel = await readFile(panelUrl, "utf8");
const cards = await readFile(cardsUrl, "utf8");
const drawer = await readFile(drawerUrl, "utf8");

test("scheduled task cards expose a dedicated detail affordance", () => {
  assert.match(panel, /className="cron-card-actions-cluster"/);
  assert.match(panel, /className="cron-card-detail-btn"/);
  assert.match(panel, /onClick=\{\(\) => openJobDrawer\(job\)\}/);
  assert.match(cards, /\.cron-card-detail-btn\s*\{/);
});

test("task drawer combines configuration, actions, and recent runs", () => {
  assert.match(panel, /className="cron-job-drawer"/);
  assert.match(panel, /className="cron-job-overview-card"/);
  assert.match(panel, /className="cron-job-drawer-actions"/);
  assert.match(panel, /drawerJobRuns\.slice\(0, 30\)/);
  assert.match(panel, /onClick=\{\(\) => openRunDrawer\(run\)\}/);
  assert.match(panel, /backdropFilter: "none"/);
  assert.match(drawer, /@media \(prefers-reduced-motion: reduce\)/);
});

test("task drawer refreshes its own run history after actions", () => {
  assert.match(panel, /const loadDrawerJobRuns = useCallback/);
  assert.match(panel, /if \(drawerJobId === job\.id\)/);
  assert.match(panel, /if \(drawerJobId === run\.job_id\)/);
});
