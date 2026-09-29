/** Exercise actual writers/readers in separate processes; no provider requests. */
import { mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const cwd = fileURLToPath(new URL("../", import.meta.url));
const root = realpathSync(mkdtempSync(join(tmpdir(), "astro-settings-restart-")));
const env = { ...process.env, ASTRO_MEMORY_DIR: root, ASTRO_SETTINGS_RESTART_ROOT: root, OPENAI_API_KEY: "storage-fixture-placeholder-not-a-real-key" };
const cases = [
  ["astro-harness", "commands::providers::core::tests::settings_restart_write_fixture"],
  ["astro-harness", "commands::providers::core::tests::settings_restart_read_fixture"],
  ["server", "cron_runner::tests::settings_restart_read_fixture"],
  ["workflow", "engine::tests::settings_restart_read_fixture"],
];
try {
  console.log("Compiling one consistent test snapshot for all restart phases");
  const build = spawnSync("cargo", ["test", "-p", "astro-harness", "-p", "server", "-p", "workflow", "--lib", "--no-run", "--message-format=json"],
    { cwd, env, encoding: "utf8", timeout: 1_200_000, maxBuffer: 32 * 1024 * 1024 });
  if (build.error || build.status !== 0) {
    process.stderr.write(build.stderr ?? "");
    throw build.error ?? new Error("Restart fixture compilation failed");
  }
  const executables = new Map();
  for (const line of build.stdout.split("\n").filter(Boolean)) {
    const artifact = JSON.parse(line);
    if (artifact.reason === "compiler-artifact" && artifact.profile?.test && artifact.executable) {
      executables.set(artifact.target.name, artifact.executable);
    }
  }
  for (const [pkg, name] of cases) {
    console.log(`Verifying ${pkg}: ${name.split("::").at(-1)}`);
    const executable = executables.get(pkg === "astro-harness" ? "astro_agent_lib" : pkg);
    if (!executable) throw new Error(`Missing test executable: ${pkg}`);
    const run = spawnSync(executable, [name, "--ignored", "--exact", "--nocapture"],
      { cwd, env, encoding: "utf8", timeout: 1_200_000, maxBuffer: 16 * 1024 * 1024 });
    if (run.error || run.status !== 0 || !/test result: ok\. 1 passed;/.test(run.stdout)) {
      process.stderr.write(run.stdout ?? ""); process.stderr.write(run.stderr ?? "");
      throw run.error ?? new Error(`Restart verification failed: ${pkg}`);
    }
  }
  console.log("PASS: saved Desktop settings survived process restart and matched Desktop/Cron/Workflow readers.");
} finally {
  // Only this runner's mkdtemp directory is removed; real Astro data is never targeted.
  rmSync(root, { recursive: true, force: true });
}
