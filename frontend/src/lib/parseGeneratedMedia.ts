/**
 * 从工具结果文本中解析生成媒体路径（image_gen / video_gen / tts / music_gen 固定文案）。
 */
export type GeneratedMediaKind = "image" | "video" | "audio" | "html";

export type GeneratedMedia = {
  kind: GeneratedMediaKind;
  path: string;
};

const LABELED =
  /(?:图片|视频|语音|音乐)已生成[：:]\s*(\S+)/g;

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
  if (line.includes("语音已生成") || line.includes("音乐已生成")) return "audio";
  return null;
}

function kindFromPath(path: string): GeneratedMediaKind | null {
  return EXT_KIND[extOf(path)] ?? null;
}

/** 从工具 output 文本提取媒体条目（去重，保序） */
export function parseGeneratedMedia(text: string | null | undefined): GeneratedMedia[] {
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
