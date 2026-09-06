import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const hook = await readFile(
  new URL("../../hooks/ui/useTransientToast.ts", import.meta.url),
  "utf8",
);
const toast = await readFile(
  new URL("../../components/ui/Toast.tsx", import.meta.url),
  "utf8",
);
const styles = await readFile(
  new URL("../../styles/components/toast.css", import.meta.url),
  "utf8",
);

test("transient toast forwards an optional action", () => {
  assert.match(hook, /actionLabel\?: string/);
  assert.match(hook, /onAction\?: \(\) => void \| Promise<void>/);
  assert.match(hook, /setOnAction\(\(\) => opts\?\.onAction\)/);
  assert.match(toast, /className="astro-toast-action"/);
  assert.match(toast, /void onAction\(\)/);
  assert.match(styles, /\.astro-toast-action\s*\{/);
});
