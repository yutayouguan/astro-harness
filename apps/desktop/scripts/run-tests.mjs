import { readdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const sourceRoot = fileURLToPath(new URL("../src/", import.meta.url));

function findTests(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return findTests(path);
    return /\.test\.(?:mjs|ts)$/.test(entry.name) ? [path] : [];
  });
}

const tests = findTests(sourceRoot).sort();
if (tests.length === 0) {
  console.error("No frontend tests found under src/.");
  process.exit(1);
}

const result = spawnSync(process.execPath, ["--test", ...tests], {
  stdio: "inherit",
});

if (result.error) throw result.error;
process.exit(result.status ?? 1);
