#!/usr/bin/env node
// 统一 Astro Agent 的版本号：tauri.conf.json、src-tauri/Cargo.toml、package.json、Cargo.lock。
//
// 用法：
//   node tools/bump-version.mjs 0.2.0     # 写入新版本号并同步 Cargo.lock
//   node tools/bump-version.mjs --check   # 只校验四处一致（CI / 发布前置检查用）
//
// 发布流程固定为：先 bump 版本号并提交，再打同名 tag（v0.2.0），
// 这样 tauri-action 的 `v__VERSION__` 与安装包里记录的版本一定一致。

import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const TAURI_CONF = 'apps/desktop/src-tauri/tauri.conf.json';
const TAURI_CARGO = 'apps/desktop/src-tauri/Cargo.toml';
const SERVER_CARGO = 'crates/agent-server/Cargo.toml';
const DESKTOP_PACKAGE = 'apps/desktop/package.json';
const CARGO_LOCK = 'Cargo.lock';

const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?$/;

const read = (relative) => readFileSync(path.join(repoRoot, relative), 'utf8');

// 每个文件的版本号读写方式：json 直接改字段，Cargo 文件只改包自身的那一行。
const targets = [
  {
    file: TAURI_CONF,
    read: (text) => JSON.parse(text).version,
    write: (text, version) => {
      const parsed = JSON.parse(text);
      parsed.version = version;
      return `${JSON.stringify(parsed, null, 2)}\n`;
    },
  },
  {
    file: DESKTOP_PACKAGE,
    read: (text) => JSON.parse(text).version,
    write: (text, version) => {
      const parsed = JSON.parse(text);
      parsed.version = version;
      return `${JSON.stringify(parsed, null, 2)}\n`;
    },
  },
  {
    file: TAURI_CARGO,
    read: (text) => {
      const match = text.match(/\[package\][\s\S]*?\nversion = "([^"]+)"/);
      if (!match) throw new Error(`${TAURI_CARGO} 里没有找到 [package] version`);
      return match[1];
    },
    write: (text, version) =>
      text.replace(/(\[package\][\s\S]*?\nversion = ")[^"]+(")/, `$1${version}$2`),
  },
  {
    // 后端启动日志会打印 `server` crate 的版本，需要跟着应用版本一起走。
    file: SERVER_CARGO,
    read: (text) => {
      const match = text.match(/\[package\][\s\S]*?\nversion = "([^"]+)"/);
      if (!match) throw new Error(`${SERVER_CARGO} 里没有找到 [package] version`);
      return match[1];
    },
    write: (text, version) =>
      text.replace(/(\[package\][\s\S]*?\nversion = ")[^"]+(")/, `$1${version}$2`),
  },
  {
    file: CARGO_LOCK,
    read: (text) => {
      const astroAgent = text.match(/\[\[package\]\]\nname = "astro-agent"\nversion = "([^"]+)"/);
      const server = text.match(/\[\[package\]\]\nname = "server"\nversion = "([^"]+)"/);
      if (!astroAgent || !server) {
        throw new Error(`${CARGO_LOCK} 里没有找到 astro-agent / server 的 package 条目`);
      }
      if (astroAgent[1] !== server[1]) {
        throw new Error(
          `${CARGO_LOCK} 里 astro-agent (${astroAgent[1]}) 与 server (${server[1]}) 版本不一致`,
        );
      }
      return astroAgent[1];
    },
    write: (text, version) =>
      text
        .replace(
          /(\[\[package\]\]\nname = "astro-agent"\nversion = ")[^"]+(")/,
          `$1${version}$2`,
        )
        .replace(/(\[\[package\]\]\nname = "server"\nversion = ")[^"]+(")/, `$1${version}$2`),
  },
];

function fail(message) {
  console.error(message);
  process.exit(1);
}

function readVersions() {
  return targets.map((target) => ({ file: target.file, version: target.read(read(target.file)) }));
}

const versions = readVersions();
const reference = versions.find((entry) => entry.file === TAURI_CONF).version;

if (process.argv.includes('--check')) {
  const mismatched = versions.filter((entry) => entry.version !== reference);
  if (mismatched.length > 0) {
    const detail = versions.map((entry) => `  ${entry.file}: ${entry.version}`).join('\n');
    fail(
      `版本号不一致，以 ${TAURI_CONF} (${reference}) 为准：\n${detail}\n先执行 node tools/bump-version.mjs <版本号>`,
    );
  }
  console.log(`版本号一致：${reference}`);
  process.exit(0);
}

const next = process.argv[2];
if (!next) {
  fail('用法：node tools/bump-version.mjs <版本号>  或  node tools/bump-version.mjs --check');
}
if (!SEMVER.test(next)) {
  fail(`版本号 ${next} 不是合法的 semver（示例：0.2.0 / 0.2.0-beta.1）`);
}

for (const target of targets) {
  const absolute = path.join(repoRoot, target.file);
  writeFileSync(absolute, target.write(read(target.file), next));
}

const mismatchedAfter = readVersions().filter((entry) => entry.version !== next);
if (mismatchedAfter.length > 0) {
  fail(`写入后仍有版本号不一致：${mismatchedAfter.map((entry) => entry.file).join(', ')}`);
}

console.log(`版本号已从 ${reference} 更新为 ${next}：`);
for (const target of targets) {
  console.log(`  ${target.file}`);
}
