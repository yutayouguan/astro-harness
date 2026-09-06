import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  LOOP_TEMPLATES,
  loopTemplateCopy,
  type LoopTemplate,
} from "./loopTemplates.ts";

test("localized template copy uses the catalog fields for both languages", () => {
  const template = LOOP_TEMPLATES[0] as LoopTemplate;

  assert.deepEqual(loopTemplateCopy(template, "zh"), {
    name: template.name,
    description: template.description,
  });
  assert.deepEqual(loopTemplateCopy(template, "en"), {
    name: template.nameEn,
    description: template.descriptionEn,
  });
});

test("quick start and template picker share the complete template grid", async () => {
  const source = await readFile(
    new URL("LoopPanel.tsx", import.meta.url),
    "utf8",
  );
  const uses = source.match(/\{renderTemplateGrid\(\)\}/g) ?? [];

  assert.ok(LOOP_TEMPLATES.length > 3);
  assert.equal(uses.length, 2);
  assert.match(source, /LOOP_TEMPLATES\.map\(\(tpl\)/);
  assert.doesNotMatch(source, /LOOP_TEMPLATES\.slice/);
});
