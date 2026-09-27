#!/usr/bin/env node
// 基线检查：workspace 测试 + 前端单测 + 类型检查 + 样式检查；
// `--full` 追加 Playwright 全量（chromium + webkit）。
//
// 与 tools/verify-config-native.mjs 等一致，失败即非零退出，便于本地/CI 直接调用。
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const desktop = resolve(root, "apps/desktop");
const full = process.argv.includes("--full");

const steps = [
  {
    name: "cargo test --workspace",
    cwd: root,
    command: "cargo",
    args: ["test", "--workspace", "--no-fail-fast"],
  },
  {
    name: "frontend unit tests",
    cwd: desktop,
    command: "node",
    args: ["scripts/run-tests.mjs"],
  },
  {
    name: "typescript",
    cwd: desktop,
    command: "npx",
    args: ["tsc", "--noEmit"],
  },
  {
    name: "stylelint",
    cwd: desktop,
    command: "npx",
    args: ["stylelint", "src/**/*.css"],
  },
  {
    name: "css layer order",
    cwd: desktop,
    command: "node",
    args: ["scripts/check-style-layers.mjs"],
  },
  ...(full
    ? [
        {
          name: "playwright (chromium + webkit)",
          cwd: desktop,
          command: "npx",
          args: ["playwright", "test"],
        },
      ]
    : []),
];

const failed = [];
for (const step of steps) {
  process.stdout.write(`\n▶ ${step.name}\n`);
  const result = spawnSync(step.command, step.args, {
    cwd: step.cwd,
    stdio: "inherit",
    env: process.env,
  });
  if (result.error) {
    process.stderr.write(`\n✗ ${step.name}: ${result.error.message}\n`);
    failed.push(step.name);
    continue;
  }
  if (result.status !== 0) failed.push(step.name);
}

const scope = full ? "（含 Playwright 全量）" : "（quick：不含 Playwright）";
if (failed.length === 0) {
  process.stdout.write(`\n✓ 基线检查通过 ${scope}\n`);
  process.exit(0);
}
process.stderr.write(
  `\n✗ 基线检查失败 ${scope}：${failed.join(" / ")}\n` +
    "提示：只跑 Rust 用 `cargo test --workspace`；Playwright 全量在 apps/desktop 下 `npx playwright test`（或用本脚本 `--full`）。\n",
);
process.exit(1);
