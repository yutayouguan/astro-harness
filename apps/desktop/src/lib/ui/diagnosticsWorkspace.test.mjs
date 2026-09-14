import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [panel, hook, storage, cleanup, css] = await Promise.all([
  read("../../components/settings/DiagnosticsPanel.tsx"),
  read("../../hooks/settings/useDiagnosticsSettings.ts"),
  read("../../components/settings/StorageDiagnostics.tsx"),
  read("../../components/settings/StorageCleanup.tsx"),
  read("../../styles/features/settings/diagnostics-workspace.css"),
]);

test("diagnostics defaults to logs and preserves both tab subtrees", () => {
  assert.match(panel, /\[tab, setTab\] = useState\("logs"\)/);
  assert.match(panel, /<SegmentedTabs/);
  assert.match(panel, /role="tabpanel"[\s\S]*hidden=\{tab !== "logs"\}/);
  assert.match(panel, /hidden=\{tab !== "storage"\}/);
  assert.match(panel, /active=\{active && tab === "storage"\}/);
  assert.match(panel, /logsActive: active && tab === "logs"/);
  assert.match(css, /\.diagnostics-workspace \[hidden\] \{ display: none; \}/);
});

test("offscreen logs do not poll and retired requests cannot overwrite a newer view", () => {
  assert.match(hook, /if \(!active \|\| !logsActive\) return/);
  assert.match(hook, /if \(!active \|\| !logsActive \|\| !liveLogs\) return/);
  assert.match(hook, /generation !== logRequestGenerationRef.current/);
  assert.match(hook, /generation === statusRequestGenerationRef.current/);
  assert.match(hook, /\[active, logsActive, activeSessionId\]/);
  assert.match(hook, /if \(!hasSession\) setScope\("all"\)/);
});

test("row limits stay in advanced filters and log details preserve raw text", () => {
  const advanced = panel.indexOf('className="diagnostics-advanced"');
  assert.ok(advanced > 0);
  assert.ok(panel.indexOf("LINE_PRESETS.map", advanced) > advanced);
  assert.match(panel, /navigator.clipboard.writeText\(selectedLog.raw\)/);
  assert.match(panel, /\{selectedLog.raw\}/);
  assert.match(panel, /sameDiagnosticLog\(selectedLog, row\)/);
  assert.match(panel, /closeButton.current\?\.focus\(\)/);
  assert.match(panel, /event.key === "Escape"/);
  assert.doesNotMatch(panel, /dangerouslySetInnerHTML/);
  assert.match(panel, /className="diagnostics-search" data-input-surface/);
  assert.match(panel, /data-active=\{advancedCount > 0 \|\| undefined\}/);
});

test("cleanup confirmation stays on its original gated path", () => {
  assert.match(storage, /const changeCleanupBusy = useCallback/);
  assert.match(storage, /onBusyChange=\{changeCleanupBusy\}/);
  assert.match(panel, /onBusyChange=\{setStorageBusy\}/);
  assert.match(panel, /disabled: storageBusy/);
  assert.match(cleanup, /confirmOnEnter=\{false\}/);
  assert.match(cleanup, /canConfirmCleanup/);
  assert.match(cleanup, /!activeRef.current/);
  assert.doesNotMatch(
    panel + storage,
    /invoke\("(?:execute_storage_cleanup|prepare_storage_cleanup)"/,
  );
});

test("layout uses bounded scrolling and no animated keyboard tabs", () => {
  assert.match(css, /\.diagnostics-log-list[^}]*overflow: auto/);
  assert.match(css, /\.diagnostics-storage-panel[^}]*overflow: auto/);
  assert.match(css, /\.diagnostics-tabs button \{ transition: none; \}/);
  assert.match(css, /@container \(max-width: 850px\)/);
  assert.match(css, /prefers-reduced-transparency: reduce/);
  assert.match(css, /prefers-contrast: more/);
});
