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

const panel = await readFile(panelUrl, "utf8");
const cards = await readFile(cardsUrl, "utf8");
const drawer = await readFile(drawerUrl, "utf8");
const dialog = await readFile(dialogUrl, "utf8");
const overlay = await readFile(overlayUrl, "utf8");

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

test("delete confirmation stays above the open task drawer", () => {
  const dialogLayer = Number(dialog.match(/--z-dialog:\s*(\d+)/)?.[1]);
  const overlayLayers = [
    "z-overlay-drawer",
    "z-overlay-modal",
    "z-overlay-popover",
  ].map((name) => Number(overlay.match(new RegExp(`--${name}:\\s*(\\d+)`))?.[1]));

  assert.ok(Number.isFinite(dialogLayer));
  assert.ok(overlayLayers.every(Number.isFinite));
  assert.ok(
    overlayLayers.every((layer) => dialogLayer > layer),
    "confirmation dialog must render above every shared overlay",
  );
});
