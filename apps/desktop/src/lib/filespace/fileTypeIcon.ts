/**
 * 按扩展名 / 文件名解析 Lucide 图标与展示 kind。
 * 工作区与文件空间共用，保证同类后缀直接显示对应图标。
 */
import type { LucideIcon } from "lucide-react";
import {
  Baseline,
  Binary,
  BookText,
  Braces,
  Coffee,
  Database,
  File,
  FileArchive,
  FileAudio,
  FileCode2,
  FileCog,
  FileImage,
  FileJson,
  FileLock2,
  FileSpreadsheet,
  FileText,
  FileType,
  FileVideo,
  Folder,
  Globe,
  NotebookText,
  Package,
  Palette,
  Presentation,
  ScrollText,
  Terminal,
} from "lucide-react";

/** 图标色板 / 语义种类（用于 data-kind） */
export type FileGlyphKind =
  | "folder"
  | "md"
  | "json"
  | "config"
  | "db"
  | "lock"
  | "text"
  | "image"
  | "video"
  | "audio"
  | "archive"
  | "pdf"
  | "word"
  | "sheet"
  | "slides"
  | "code"
  | "code-js"
  | "code-ts"
  | "code-py"
  | "code-rs"
  | "code-go"
  | "code-java"
  | "code-c"
  | "code-shell"
  | "code-web"
  | "code-sql"
  | "notebook"
  | "font"
  | "binary"
  | "package"
  | "unknown";

/** 内置打开方式 */
export type FileOpenMode =
  | "folder"
  | "text"
  | "media-image"
  | "media-video"
  | "media-audio"
  | "media-pdf"
  | "html-preview"
  | "external";

export type ResolvedFileType = {
  kind: FileGlyphKind;
  Icon: LucideIcon;
  open: FileOpenMode;
};

function entry(
  kind: FileGlyphKind,
  Icon: LucideIcon,
  open: FileOpenMode = "text",
): ResolvedFileType {
  return { kind, Icon, open };
}

/** 无扩展名或特殊 basename */
const BASENAME_MAP: Record<string, ResolvedFileType> = {
  dockerfile: entry("package", Package),
  makefile: entry("config", FileCog),
  gnumakefile: entry("config", FileCog),
  cmakelists: entry("config", FileCog),
  license: entry("text", ScrollText),
  licence: entry("text", ScrollText),
  readme: entry("md", BookText),
  changelog: entry("text", ScrollText),
  authors: entry("text", FileText),
  gemfile: entry("code", FileCode2),
  rakefile: entry("code", FileCode2),
  procfile: entry("config", FileCog),
  vagrantfile: entry("config", FileCog),
};

/** 扩展名 → 图标（无点、小写） */
const EXT_MAP: Record<string, ResolvedFileType> = {
  // 文档 / Markdown
  md: entry("md", BookText),
  mdx: entry("md", BookText),
  markdown: entry("md", BookText),
  txt: entry("text", FileText),
  log: entry("text", FileText),
  rtf: entry("text", FileText, "external"),
  rst: entry("text", FileText),
  adoc: entry("text", FileText),
  org: entry("text", FileText),

  // 数据 / 配置
  json: entry("json", FileJson),
  jsonc: entry("json", FileJson),
  json5: entry("json", FileJson),
  yaml: entry("config", FileCog),
  yml: entry("config", FileCog),
  toml: entry("config", FileCog),
  ini: entry("config", FileCog),
  conf: entry("config", FileCog),
  cfg: entry("config", FileCog),
  env: entry("config", FileCog),
  properties: entry("config", FileCog),
  xml: entry("config", Braces),
  plist: entry("config", Braces),
  graphql: entry("code", Braces),
  gql: entry("code", Braces),

  // 数据库
  db: entry("db", Database, "external"),
  sqlite: entry("db", Database, "external"),
  sqlite3: entry("db", Database, "external"),
  "db-shm": entry("db", Database, "external"),
  "db-wal": entry("db", Database, "external"),
  "db-journal": entry("db", Database, "external"),

  // 锁 / 密钥
  lock: entry("lock", FileLock2, "external"),
  pem: entry("lock", FileLock2, "external"),
  key: entry("lock", FileLock2, "external"),
  crt: entry("lock", FileLock2, "external"),
  cer: entry("lock", FileLock2, "external"),
  p12: entry("lock", FileLock2, "external"),
  pfx: entry("lock", FileLock2, "external"),

  // 图片
  png: entry("image", FileImage, "media-image"),
  jpg: entry("image", FileImage, "media-image"),
  jpeg: entry("image", FileImage, "media-image"),
  gif: entry("image", FileImage, "media-image"),
  webp: entry("image", FileImage, "media-image"),
  svg: entry("image", FileImage, "media-image"),
  bmp: entry("image", FileImage, "media-image"),
  ico: entry("image", FileImage, "media-image"),
  avif: entry("image", FileImage, "media-image"),
  heic: entry("image", FileImage, "media-image"),
  heif: entry("image", FileImage, "media-image"),
  tif: entry("image", FileImage, "external"),
  tiff: entry("image", FileImage, "external"),
  psd: entry("image", FileImage, "external"),
  ai: entry("image", FileImage, "external"),
  sketch: entry("image", FileImage, "external"),
  fig: entry("image", FileImage, "external"),

  // 视频
  mp4: entry("video", FileVideo, "media-video"),
  webm: entry("video", FileVideo, "media-video"),
  mov: entry("video", FileVideo, "media-video"),
  mkv: entry("video", FileVideo, "media-video"),
  avi: entry("video", FileVideo, "media-video"),
  m4v: entry("video", FileVideo, "media-video"),
  ogv: entry("video", FileVideo, "media-video"),
  flv: entry("video", FileVideo, "external"),
  wmv: entry("video", FileVideo, "external"),

  // 音频
  mp3: entry("audio", FileAudio, "media-audio"),
  wav: entry("audio", FileAudio, "media-audio"),
  flac: entry("audio", FileAudio, "media-audio"),
  aac: entry("audio", FileAudio, "media-audio"),
  ogg: entry("audio", FileAudio, "media-audio"),
  m4a: entry("audio", FileAudio, "media-audio"),
  wma: entry("audio", FileAudio, "media-audio"),
  aiff: entry("audio", FileAudio, "media-audio"),
  opus: entry("audio", FileAudio, "media-audio"),

  // 压缩 / 磁盘镜像
  zip: entry("archive", FileArchive, "external"),
  rar: entry("archive", FileArchive, "external"),
  "7z": entry("archive", FileArchive, "external"),
  tar: entry("archive", FileArchive, "external"),
  gz: entry("archive", FileArchive, "external"),
  tgz: entry("archive", FileArchive, "external"),
  bz2: entry("archive", FileArchive, "external"),
  xz: entry("archive", FileArchive, "external"),
  lz4: entry("archive", FileArchive, "external"),
  zst: entry("archive", FileArchive, "external"),
  cab: entry("archive", FileArchive, "external"),
  dmg: entry("archive", FileArchive, "external"),
  iso: entry("archive", FileArchive, "external"),
  img: entry("archive", FileArchive, "external"),
  pkg: entry("archive", Package, "external"),
  deb: entry("archive", Package, "external"),
  rpm: entry("archive", Package, "external"),
  apk: entry("archive", Package, "external"),
  ipa: entry("archive", Package, "external"),

  // 办公文档
  pdf: entry("pdf", ScrollText, "media-pdf"),
  doc: entry("word", FileText, "external"),
  docx: entry("word", FileText, "external"),
  odt: entry("word", FileText, "external"),
  pages: entry("word", FileText, "external"),
  xls: entry("sheet", FileSpreadsheet, "external"),
  xlsx: entry("sheet", FileSpreadsheet, "external"),
  xlsm: entry("sheet", FileSpreadsheet, "external"),
  ods: entry("sheet", FileSpreadsheet, "external"),
  numbers: entry("sheet", FileSpreadsheet, "external"),
  csv: entry("sheet", FileSpreadsheet),
  tsv: entry("sheet", FileSpreadsheet),
  ppt: entry("slides", Presentation, "external"),
  pptx: entry("slides", Presentation, "external"),
  odp: entry("slides", Presentation, "external"),
  keynote: entry("slides", Presentation, "external"),

  // JS / TS
  js: entry("code-js", Braces),
  mjs: entry("code-js", Braces),
  cjs: entry("code-js", Braces),
  jsx: entry("code-js", Braces),
  ts: entry("code-ts", FileType),
  tsx: entry("code-ts", FileType),
  mts: entry("code-ts", FileType),
  cts: entry("code-ts", FileType),
  vue: entry("code-web", FileCode2),
  svelte: entry("code-web", FileCode2),
  astro: entry("code-web", FileCode2),

  // Python / Rust / Go / JVM
  py: entry("code-py", FileCode2),
  pyw: entry("code-py", FileCode2),
  pyi: entry("code-py", FileCode2),
  ipynb: entry("notebook", NotebookText),
  rs: entry("code-rs", FileCode2),
  go: entry("code-go", FileCode2),
  java: entry("code-java", Coffee),
  kt: entry("code-java", Coffee),
  kts: entry("code-java", Coffee),
  scala: entry("code-java", Coffee),
  groovy: entry("code-java", Coffee),
  gradle: entry("code-java", Coffee),

  // C 系
  c: entry("code-c", FileCode2),
  h: entry("code-c", FileCode2),
  cpp: entry("code-c", FileCode2),
  cc: entry("code-c", FileCode2),
  cxx: entry("code-c", FileCode2),
  hpp: entry("code-c", FileCode2),
  hh: entry("code-c", FileCode2),
  m: entry("code-c", FileCode2),
  mm: entry("code-c", FileCode2),
  cs: entry("code-c", FileCode2),
  fs: entry("code", FileCode2),
  fsx: entry("code", FileCode2),

  // 其它语言
  rb: entry("code", FileCode2),
  php: entry("code", FileCode2),
  swift: entry("code", FileCode2),
  dart: entry("code", FileCode2),
  lua: entry("code", FileCode2),
  r: entry("code", FileCode2),
  jl: entry("code", FileCode2),
  zig: entry("code", FileCode2),
  nim: entry("code", FileCode2),
  ex: entry("code", FileCode2),
  exs: entry("code", FileCode2),
  erl: entry("code", FileCode2),
  hs: entry("code", FileCode2),
  clj: entry("code", FileCode2),
  cljs: entry("code", FileCode2),
  elm: entry("code", FileCode2),
  proto: entry("code", FileCode2),
  thrift: entry("code", FileCode2),
  wasm: entry("binary", Binary, "external"),

  // Shell / 脚本
  sh: entry("code-shell", Terminal),
  bash: entry("code-shell", Terminal),
  zsh: entry("code-shell", Terminal),
  fish: entry("code-shell", Terminal),
  ps1: entry("code-shell", Terminal),
  psm1: entry("code-shell", Terminal),
  bat: entry("code-shell", Terminal),
  cmd: entry("code-shell", Terminal),
  command: entry("code-shell", Terminal),

  // Web
  html: entry("code-web", Globe, "html-preview"),
  htm: entry("code-web", Globe, "html-preview"),
  xhtml: entry("code-web", Globe),
  css: entry("code-web", Palette),
  scss: entry("code-web", Palette),
  sass: entry("code-web", Palette),
  less: entry("code-web", Palette),
  styl: entry("code-web", Palette),

  // SQL
  sql: entry("code-sql", Database),
  prisma: entry("code-sql", Database),

  // 字体
  ttf: entry("font", Baseline, "external"),
  otf: entry("font", Baseline, "external"),
  woff: entry("font", Baseline, "external"),
  woff2: entry("font", Baseline, "external"),
  eot: entry("font", Baseline, "external"),
  icns: entry("font", Baseline, "external"),

  // 二进制 / 原生
  bin: entry("binary", Binary, "external"),
  exe: entry("binary", Binary, "external"),
  dll: entry("binary", Binary, "external"),
  dylib: entry("binary", Binary, "external"),
  so: entry("binary", Binary, "external"),
  o: entry("binary", Binary, "external"),
  a: entry("binary", Binary, "external"),
  obj: entry("binary", Binary, "external"),
  class: entry("binary", Binary, "external"),
  jar: entry("package", Package, "external"),
  war: entry("package", Package, "external"),
  ear: entry("package", Package, "external"),
};

/** 小写扩展名（无点）；无扩展名时返回空串 */
export function fileExt(name: string): string {
  const base = name.split(/[/\\]/).pop() ?? name;
  const i = base.lastIndexOf(".");
  if (i <= 0) return "";
  return base.slice(i + 1).toLowerCase();
}

/**
 * 解析文件类型图标。
 * `isDir` 时固定为文件夹图标。
 */
export function resolveFileType(name: string, isDir = false): ResolvedFileType {
  if (isDir) return entry("folder", Folder, "folder");

  const base = (name.split(/[/\\]/).pop() ?? name).toLowerCase();

  if (base === "dockerfile" || base.startsWith("dockerfile.")) {
    return entry("package", Package);
  }
  if (
    base === "makefile" ||
    base === "gnumakefile" ||
    base === "cmakelists.txt"
  ) {
    return entry("config", FileCog);
  }
  if (
    base === ".gitignore" ||
    base === ".gitattributes" ||
    base === ".editorconfig" ||
    base === ".npmrc" ||
    base === ".nvmrc" ||
    base === ".prettierrc" ||
    base === ".eslintrc" ||
    base.endsWith(".eslintrc.json") ||
    base.endsWith(".prettierrc.json")
  ) {
    return entry("config", FileCog);
  }
  if (
    base === "package.json" ||
    base === "package-lock.json" ||
    base === "pnpm-lock.yaml" ||
    base === "yarn.lock" ||
    base === "cargo.toml" ||
    base === "cargo.lock" ||
    base === "go.mod" ||
    base === "go.sum" ||
    base === "composer.json" ||
    base === "gemfile.lock"
  ) {
    return entry("package", Package);
  }

  // README / LICENSE 等（含 README.md）
  const stem = base.includes(".") ? base.slice(0, base.indexOf(".")) : base;
  if (stem === "readme") return entry("md", BookText);
  if (stem === "license" || stem === "licence")
    return entry("text", ScrollText);
  if (BASENAME_MAP[base]) return BASENAME_MAP[base];

  const ext = fileExt(name);
  if (ext && EXT_MAP[ext]) return EXT_MAP[ext];
  if (ext) return entry("text", FileText);
  return entry("unknown", File, "external");
}

/** 是否宜用系统默认应用打开（不宜内置文本编辑器） */
export function isExternalOnlyFile(name: string): boolean {
  return resolveFileType(name, false).open === "external";
}

/** 是否可在应用内嵌 PDF 预览 */
export function isPdfFile(name: string): boolean {
  return resolveFileType(name, false).open === "media-pdf";
}

/** 若可内嵌预览则返回 image / video / audio / html */
export function mediaKindOf(
  name: string,
): "image" | "video" | "audio" | "html" | null {
  const open = resolveFileType(name, false).open;
  if (open === "media-image") return "image";
  if (open === "media-video") return "video";
  if (open === "media-audio") return "audio";
  if (open === "html-preview") return "html";
  return null;
}
