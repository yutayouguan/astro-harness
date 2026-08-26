/**
 * 从 material-icon-theme 源码仓库提取文件/文件夹图标与映射表。
 *
 * 用法：node scripts/vendor-file-icons.mjs <material-icon-theme 源码目录>
 *
 * 产物：
 *   public/file-icons/*.svg                       实际用到的图标
 *   src/lib/filespace/generated/materialIconManifest.ts   名称 → 图标映射
 */
import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  copyFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { build } from "esbuild";

const HERE = dirname(fileURLToPath(import.meta.url));
const APP = resolve(HERE, "..");
const ICON_OUT = join(APP, "public", "file-icons");
const MANIFEST_OUT = join(APP, "src", "lib", "filespace", "generated", "materialIconManifest.ts");

const themeRoot = resolve(process.argv[2] ?? "");
if (!themeRoot || !existsSync(join(themeRoot, "src", "core", "icons", "fileIcons.ts"))) {
  console.error("用法: node scripts/vendor-file-icons.mjs <material-icon-theme 源码目录>");
  process.exit(1);
}

/** 用 esbuild 把主题的 TS 定义打成可直接 import 的 ESM。 */
async function loadIconDefinitions() {
  const work = mkdtempSync(join(tmpdir(), "mit-defs-"));
  const entry = join(work, "entry.mjs");
  const out = join(work, "defs.mjs");
  writeFileSync(
    entry,
    [
      `import { fileIcons } from ${JSON.stringify(join(themeRoot, "src/core/icons/fileIcons.ts"))};`,
      `import { folderIcons } from ${JSON.stringify(join(themeRoot, "src/core/icons/folderIcons.ts"))};`,
      "export const definitions = { fileIcons, folderIcons };",
    ].join("\n"),
  );
  await build({ entryPoints: [entry], outfile: out, bundle: true, platform: "node", format: "esm", logLevel: "silent" });
  const { definitions } = await import(pathToFileURL(out).href);
  rmSync(work, { recursive: true, force: true });
  return definitions;
}

/** 主题会把 `src` 同时匹配成 .src / _src / -src / __src__。 */
function folderNameVariants(name) {
  return [name, `.${name}`, `_${name}`, `-${name}`, `__${name}__`];
}

/** 从主题源码里取出内联的 path 常量，避免在这里复制一份会过期的坐标。 */
function pathConstant(file, constName) {
  const source = readFileSync(join(themeRoot, "src/core/generator", file), "utf8");
  const match = new RegExp(`const ${constName}\\s*=\\s*\n?\\s*'([^']+)'`).exec(source);
  if (!match) throw new Error(`未能在 ${file} 中找到 ${constName}`);
  return match[1];
}

const DEFAULT_COLOR = "#90a4ae";
const OPEN_FOLDER_PATH = pathConstant("folderGenerator.ts", "folderIconOpen");

/** 主题构建期才生成的基础图标：默认文件、默认文件夹及其打开态。 */
const synthesized = new Map(
  [
    ["file", pathConstant("fileGenerator.ts", "fileIcon")],
    ["folder", pathConstant("folderGenerator.ts", "folderIcon")],
    ["folder-open", OPEN_FOLDER_PATH],
  ].map(([name, d]) => [
    name,
    `<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg"><path d="${d}" fill="${DEFAULT_COLOR}" /></svg>`,
  ]),
);

/** 打开态文件夹图标同样是构建期产物：把 id="folder" 的路径换成打开态路径。 */
function openFolderVariant(closedName) {
  const svg = readFileSync(join(themeRoot, "icons", `${closedName}.svg`), "utf8");
  const tag = /<path[^>]*\bid="folder"[^>]*\/?>/.exec(svg);
  if (!tag) return null;
  const opened = tag[0].replace(/\bd="[^"]*"/, `d="${OPEN_FOLDER_PATH}"`);
  return opened === tag[0] ? null : svg.replace(tag[0], opened);
}

const onDisk = new Set(
  readdirSync(join(themeRoot, "icons"))
    .filter((file) => file.endsWith(".svg"))
    .map((file) => basename(file, ".svg")),
);

const used = new Set();
/** 只登记能落地的图标；克隆生成的图标在源码里没有 svg，跳过后回退默认图标。 */
function useIcon(name) {
  if (!name) return null;
  if (!onDisk.has(name) && !synthesized.has(name)) {
    const closed = name.endsWith("-open") ? name.slice(0, -"-open".length) : null;
    if (!closed || !onDisk.has(closed)) return null;
    const svg = openFolderVariant(closed);
    if (!svg) return null;
    synthesized.set(name, svg);
  }
  used.add(name);
  return name;
}

const definitions = await loadIconDefinitions();

const fileNames = {};
const fileExtensions = {};
for (const icon of definitions.fileIcons.icons ?? []) {
  // 跳过依赖 icon pack（angular/react/vue 等）开关的条目，保持中立映射
  if (icon.disabled || icon.enabledFor) continue;
  const name = useIcon(icon.name);
  if (!name) continue;
  for (const fileName of icon.fileNames ?? []) fileNames[fileName.toLowerCase()] = name;
  for (const ext of icon.fileExtensions ?? []) fileExtensions[ext.toLowerCase()] = name;
}

const specific = (definitions.folderIcons ?? []).find((theme) => theme.name === "specific");
const folderNames = {};
const folderNamesExpanded = {};
for (const icon of specific?.icons ?? []) {
  if (icon.disabled || icon.enabledFor) continue;
  const name = useIcon(icon.name);
  if (!name) continue;
  const open = useIcon(`${icon.name}-open`);
  for (const raw of [...(icon.folderNames ?? []), ...(icon.rootFolderNames ?? [])]) {
    for (const variant of folderNameVariants(raw.toLowerCase())) {
      folderNames[variant] = name;
      if (open) folderNamesExpanded[variant] = open;
    }
  }
}

const defaultFolder = specific?.defaultIcon?.name ?? "folder";
const defaults = {
  file: useIcon(definitions.fileIcons.defaultIcon?.name ?? "file"),
  folder: useIcon(defaultFolder),
  folderExpanded: useIcon(`${defaultFolder}-open`),
};
for (const [key, value] of Object.entries(defaults)) {
  if (!value) throw new Error(`缺少默认图标: ${key}`);
}

mkdirSync(ICON_OUT, { recursive: true });
for (const stale of readdirSync(ICON_OUT).filter((file) => file.endsWith(".svg"))) {
  if (!used.has(basename(stale, ".svg"))) rmSync(join(ICON_OUT, stale));
}
for (const name of used) {
  const generated = synthesized.get(name);
  if (generated) writeFileSync(join(ICON_OUT, `${name}.svg`), generated);
  else copyFileSync(join(themeRoot, "icons", `${name}.svg`), join(ICON_OUT, `${name}.svg`));
}

const sorted = (record) =>
  Object.fromEntries(Object.entries(record).sort(([a], [b]) => a.localeCompare(b)));

const version = (() => {
  try {
    return execFileSync("git", ["-C", themeRoot, "rev-parse", "--short", "HEAD"], { encoding: "utf8" }).trim();
  } catch {
    return "unknown";
  }
})();

mkdirSync(dirname(MANIFEST_OUT), { recursive: true });
writeFileSync(
  MANIFEST_OUT,
  `/**
 * 由 scripts/vendor-file-icons.mjs 生成，请勿手改。
 * 图标来源：material-icon-theme (MIT)，源码版本 ${version}。
 */
export type MaterialIconManifest = {
  defaults: { file: string; folder: string; folderExpanded: string };
  fileNames: Record<string, string>;
  fileExtensions: Record<string, string>;
  folderNames: Record<string, string>;
  folderNamesExpanded: Record<string, string>;
};

export const materialIconManifest: MaterialIconManifest = ${JSON.stringify(
    {
      defaults,
      fileNames: sorted(fileNames),
      fileExtensions: sorted(fileExtensions),
      folderNames: sorted(folderNames),
      folderNamesExpanded: sorted(folderNamesExpanded),
    },
    null,
    2,
  )};
`,
);

console.log(
  `icons: ${used.size} → public/file-icons\n` +
    `fileNames: ${Object.keys(fileNames).length}, fileExtensions: ${Object.keys(fileExtensions).length}, folderNames: ${Object.keys(folderNames).length}`,
);
