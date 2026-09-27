import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const repoRoot = fileURLToPath(new URL("../../../../../", import.meta.url));
const commandsRoot = join(repoRoot, "apps/desktop/src-tauri/src/commands");
const frontendRoot = join(repoRoot, "apps/desktop/src");

/**
 * 允许「暂无前端调用者」的破坏性命令；新增条目必须写明原因。
 * 典型合法场景：能力只由 gRPC/服务端契约消费（如线程附件），
 * 或命令是给尚未落地的 UI 预留的、且已单独评审。
 */
const ALLOWED_WITHOUT_CALLER = new Map();

const DESTRUCTIVE = /(delete|remove|uninstall|clear|discard|purge|forget)/;
const COMMAND_DECLARATION =
  /#\[tauri::command\][\s\S]{0,200}?pub (?:async )?fn ([a-z0-9_]+)\s*\(/g;

async function walk(directory, keep) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await walk(path, keep)));
    else if (keep(path)) files.push(path);
  }
  return files;
}

/** 收集破坏性 Tauri 命令名（去重，按名字排序）。 */
async function destructiveCommands() {
  const files = await walk(commandsRoot, (path) => path.endsWith(".rs"));
  const names = new Set();
  for (const file of files) {
    const source = await readFile(file, "utf8");
    for (const match of source.matchAll(COMMAND_DECLARATION)) {
      const name = match[1];
      if (DESTRUCTIVE.test(name)) names.add(name);
    }
  }
  return [...names].sort();
}

test("every destructive Tauri command is reachable from the frontend", async () => {
  const files = await walk(frontendRoot, (path) =>
    /\.(ts|tsx|mjs)$/.test(path),
  );
  const frontendSource = (
    await Promise.all(files.map((file) => readFile(file, "utf8")))
  ).join("\n");

  const orphans = (await destructiveCommands()).filter(
    (name) =>
      !ALLOWED_WITHOUT_CALLER.has(name) &&
      !frontendSource.includes(`"${name}"`),
  );

  assert.deepEqual(
    orphans,
    [],
    "破坏性 Tauri 命令必须在前端有调用者；没有就把命令删掉，" +
      `或写进 ALLOWED_WITHOUT_CALLER 并说明原因。当前孤儿命令：${orphans.join(", ")}`,
  );
});

test("the destructive command scan actually finds commands", async () => {
  const names = await destructiveCommands();
  assert.ok(
    names.includes("delete_session_permanently"),
    `扫描结果异常：${names.join(", ")}`,
  );
});
