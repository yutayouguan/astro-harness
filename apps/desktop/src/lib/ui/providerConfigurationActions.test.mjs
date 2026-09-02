import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const providersPanel = await readFile(
  new URL("../../components/settings/ProvidersPanel.tsx", import.meta.url),
  "utf8",
);
const providerStyles = await readFile(
  new URL("../../styles/features/providers.css", import.meta.url),
  "utf8",
);

test("provider configuration separates status, secondary actions, and saving", () => {
  const headerActions = providersPanel.match(
    /<div className="providers-pane-head-actions">([\s\S]*?)<\/PopoverSurface>\s*<\/div>/,
  )?.[1];

  assert.ok(headerActions, "provider header actions should be present");
  assert.match(headerActions, /role="switch"/);
  assert.match(headerActions, /aria-checked=\{draft\.enabled\}/);
  assert.match(headerActions, /<MoreHorizontal/);
  assert.match(headerActions, /role="menu"/);
  assert.match(headerActions, /providers-actions-menu-item is-danger/);
  assert.doesNotMatch(headerActions, /active=\{providerActionsOpen\}/);
  assert.doesNotMatch(headerActions, /<IconSave/);
  assert.doesNotMatch(providersPanel, /<IconPower/);
});

test("provider save actions live at the form boundary and reflect dirty state", () => {
  assert.match(
    providersPanel,
    /const hasUnsavedChanges = useMemo\([\s\S]*?providerSaveInput\(selected, draft\)[\s\S]*?draftFromProvider\(selected\)/,
  );
  assert.match(
    providersPanel,
    /className="providers-form-actions"[\s\S]*?providers\.discardChanges[\s\S]*?disabled=\{!hasUnsavedChanges\}[\s\S]*?providers\.saveChanges/,
  );
  assert.match(
    providerStyles,
    /\.providers-form-actions\s*\{[\s\S]*?position:\s*sticky;[\s\S]*?bottom:\s*-1px;/,
  );
});

test("provider actions expose immediate and reduced-motion feedback", () => {
  assert.match(
    providerStyles,
    /\.providers-enabled-control:active:not\(:disabled\)\s*\{[\s\S]*?transform:\s*scale\(0\.98\);/,
  );
  assert.match(
    providerStyles,
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.providers-switch-thumb[\s\S]*?transition:\s*none;/,
  );
});
