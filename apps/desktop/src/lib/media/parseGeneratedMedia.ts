/**
 * 从工具结果文本中解析生成媒体路径（image_gen / video_gen / tts / music_gen 固定文案）。
 */
export type GeneratedMediaKind = "image" | "video" | "audio" | "html" | "code";

export type GeneratedMedia = {
  kind: GeneratedMediaKind;
  path: string;
};

/** 可按代码块渲染（语法高亮）的文本/代码文件后缀。不含媒体/HTML/SVG（另有归类）。 */
export const CODE_EXT: ReadonlySet<string> = new Set([
  "txt",
  "text",
  "log",
  "md",
  "markdown",
  "mdx",
  "rst",
  "json",
  "jsonl",
  "json5",
  "yaml",
  "yml",
  "toml",
  "ini",
  "cfg",
  "conf",
  "env",
  "properties",
  "csv",
  "tsv",
  "js",
  "mjs",
  "cjs",
  "jsx",
  "ts",
  "tsx",
  "cts",
  "mts",
  "rs",
  "py",
  "pyi",
  "rb",
  "go",
  "java",
  "kt",
  "kts",
  "scala",
  "swift",
  "dart",
  "c",
  "h",
  "cc",
  "cpp",
  "cxx",
  "hpp",
  "hh",
  "hxx",
  "m",
  "mm",
  "cs",
  "php",
  "pl",
  "pm",
  "lua",
  "r",
  "jl",
  "hs",
  "ex",
  "exs",
  "erl",
  "clj",
  "cljs",
  "css",
  "scss",
  "sass",
  "less",
  "xml",
  "vue",
  "svelte",
  "astro",
  "sh",
  "bash",
  "zsh",
  "fish",
  "ksh",
  "ps1",
  "bat",
  "cmd",
  "sql",
  "graphql",
  "gql",
  "proto",
  "dockerfile",
  "makefile",
  "cmake",
  "gradle",
  "diff",
  "patch",
]);

/** 无扩展名但常见的纯文本文件名（小写整名匹配）。 */
const CODE_BASENAME: ReadonlySet<string> = new Set([
  "dockerfile",
  "makefile",
  "cmakelists.txt",
  "readme",
  "license",
  ".gitignore",
  ".env",
]);

/** 判断路径是否应作为代码/文本文件（语法高亮）渲染。 */
export function isCodePath(path: string): boolean {
  const base = (path.split(/[\\/]/).pop() ?? path).toLowerCase();
  if (CODE_BASENAME.has(base)) return true;
  return CODE_EXT.has(extOf(path));
}

const LABELED = /(?:图片|视频|语音|音乐)已生成[：:]\s*(\S+)/g;

const EXT_KIND: Record<string, GeneratedMediaKind> = {
  png: "image",
  jpg: "image",
  jpeg: "image",
  webp: "image",
  gif: "image",
  bmp: "image",
  svg: "image",
  avif: "image",
  mp4: "video",
  webm: "video",
  mov: "video",
  mkv: "video",
  m4v: "video",
  wav: "audio",
  mp3: "audio",
  m4a: "audio",
  aac: "audio",
  ogg: "audio",
  flac: "audio",
  opus: "audio",
  html: "html",
  htm: "html",
};

function extOf(path: string): string {
  const base = path.split(/[\\/]/).pop() ?? path;
  const i = base.lastIndexOf(".");
  return i >= 0 ? base.slice(i + 1).toLowerCase() : "";
}

function kindFromLabel(line: string): GeneratedMediaKind | null {
  if (line.includes("图片已生成")) return "image";
  if (line.includes("视频已生成")) return "video";
  if (line.includes("语音已生成") || line.includes("音乐已生成"))
    return "audio";
  return null;
}

function kindFromPath(path: string): GeneratedMediaKind | null {
  return EXT_KIND[extOf(path)] ?? null;
}

/** 从工具 output 文本提取媒体条目（去重，保序）；也识别 `astro_media_v1:` sidecar。 */
export function parseGeneratedMedia(
  text: string | null | undefined,
): GeneratedMedia[] {
  if (!text) return [];
  const seen = new Set<string>();
  const out: GeneratedMedia[] = [];

  const push = (kind: GeneratedMediaKind, path: string) => {
    const p = path.trim().replace(/^["']|["']$/g, "");
    if (!p || seen.has(p)) return;
    seen.add(p);
    out.push({ kind, path: p });
  };

  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed.startsWith("astro_media_v1:")) {
      try {
        const parsed = JSON.parse(
          trimmed.slice("astro_media_v1:".length),
        ) as Array<{
          kind?: string;
          reference?:
            { WorkspacePath?: string; workspace_path?: string } | string;
          // serde externally tagged enum serializes as {"workspace_path":"..."}
        }>;
        for (const item of parsed) {
          const kind =
            item.kind === "image" ||
            item.kind === "video" ||
            item.kind === "audio"
              ? (item.kind as GeneratedMediaKind)
              : null;
          let path = "";
          const ref = item.reference as
            Record<string, string> | string | undefined;
          if (typeof ref === "string") path = ref;
          else if (ref && typeof ref === "object") {
            path =
              ref.workspace_path ||
              ref.WorkspacePath ||
              ref.data_url ||
              ref.remote_uri ||
              "";
          }
          if (kind && path) push(kind, path);
        }
      } catch {
        // ignore malformed sidecar
      }
      continue;
    }
    LABELED.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = LABELED.exec(line)) !== null) {
      const path = m[1];
      const labeled = kindFromLabel(line);
      const byExt = kindFromPath(path);
      const kind = labeled ?? byExt;
      if (kind) push(kind, path);
    }
  }

  if (out.length > 0) return out;

  // 回落：任意像本地媒体路径的 token
  const pathRe =
    /(?:^|[\s"'`])((?:\/|[A-Za-z]:[\\/]|\\\\)[^\s"'`]+?\.(?:png|jpe?g|webp|gif|mp4|webm|mov|wav|mp3|m4a|html|htm))\b/gi;
  let pm: RegExpExecArray | null;
  while ((pm = pathRe.exec(text)) !== null) {
    const path = pm[1];
    const kind = kindFromPath(path);
    if (kind) push(kind, path);
  }

  return out;
}
