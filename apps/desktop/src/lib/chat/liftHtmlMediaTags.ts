/**
 * 将 Markdown 正文中的 HTML `<audio>` / `<video>` 提升为媒体引用，
 * 供 ChatMarkdown 渲染成 GeneratedMediaCard（不碰代码块内文本）。
 */

const FENCE_OR_INLINE = /(```[\s\S]*?```|`[^`\n]+`)/g;

/** `<audio src="...">` / `<video src="...">`（含自闭合与成对标签） */
const MEDIA_TAG_SRC =
  /<(audio|video)\b([^>]*?)\bsrc\s*=\s*(["'])([^"']+)\3([^>]*)>(?:\s*<\/\1\s*>)?/gi;

/** `<video ...><source src="..."></video>` */
const VIDEO_WITH_SOURCE =
  /<video\b[^>]*>\s*<source\b[^>]*\bsrc\s*=\s*(["'])([^"']+)\1[^>]*>\s*<\/video\s*>/gi;

function mediaMarkdown(kind: "audio" | "video", src: string): string {
  const path = src.trim();
  if (!path) return "";
  return `\n\n![${kind}](${path})\n\n`;
}

function liftInText(text: string): string {
  let out = text.replace(VIDEO_WITH_SOURCE, (_m, _q, src: string) =>
    mediaMarkdown("video", src),
  );
  out = out.replace(
    MEDIA_TAG_SRC,
    (_m, tag: string, _pre: string, _q: string, src: string) => {
      const kind = tag.toLowerCase() === "audio" ? "audio" : "video";
      return mediaMarkdown(kind, src);
    },
  );
  return out;
}

/** 代码块外：`<audio>` / `<video>` → `![audio|video](src)` */
export function liftHtmlMediaTags(markdown: string): string {
  if (!markdown || !/<(audio|video)\b/i.test(markdown)) return markdown;
  const parts = markdown.split(FENCE_OR_INLINE);
  return parts
    .map((part, i) => {
      // split 捕获组：奇数下标为代码块/行内代码
      if (i % 2 === 1) return part;
      return liftInText(part);
    })
    .join("");
}
