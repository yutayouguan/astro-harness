import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";

/**
 * 聊天样式面：欢迎页样式已从 markdown.css 拆到 welcome.css，
 * 断言按拼接顺序读回，后续再拆只改这里。
 */
const CHAT_CSS_FILES = ["markdown.css", "welcome.css"];

const chatCssUrl = (name) =>
  new URL(`../../styles/features/chat/${name}`, import.meta.url);

export async function readChatCss() {
  const parts = await Promise.all(
    CHAT_CSS_FILES.map((name) => readFile(chatCssUrl(name), "utf8")),
  );
  return parts.join("\n");
}

export function readChatCssSync() {
  return CHAT_CSS_FILES.map((name) =>
    readFileSync(chatCssUrl(name), "utf8"),
  ).join("\n");
}
