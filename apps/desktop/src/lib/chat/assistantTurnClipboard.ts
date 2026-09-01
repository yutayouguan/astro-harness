import type { ConversationEntry } from "../../types";
import { resolveActivityIO } from "./resolveActivityIO.ts";

/** Lightweight Markdown-to-text conversion for clipboard convenience. */
export function assistantAnswerPlainText(markdown: string): string {
  return markdown
    .replace(/```[^\n]*\n([\s\S]*?)```/g, "$1")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}#{1,6}\s+/gm, "")
    .replace(/^[ \t]*>[ \t]?/gm, "")
    .replace(/^[ \t]*[-*+][ \t]+/gm, "")
    .replace(/^[ \t]*\d+[.)][ \t]+/gm, "")
    .replace(/`([^`\n]+)`/g, "$1")
    .replace(/\*\*([^*\n]+)\*\*/g, "$1")
    .replace(/\*([^*\n]+)\*/g, "$1")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/** Export the complete reasoning/tool/answer trace as readable Markdown. */
export function assistantProcessMarkdown(message: ConversationEntry): string {
  const sections: string[] = [];
  if (message.reasoning?.trim()) {
    sections.push(`## 思考\n\n${message.reasoning.trim()}`);
  }

  const tools = (message.activities ?? []).flatMap((activity) => {
    const { input, output } = resolveActivityIO(activity);
    const details = [
      input ? `**INPUT**\n\n${input.trim()}` : "",
      output ? `**OUTPUT**\n\n${output.trim()}` : "",
    ].filter(Boolean);
    return details.length > 0
      ? [`### ${activity.title}\n\n${details.join("\n\n")}`]
      : [`### ${activity.title}`];
  });
  if (tools.length > 0) sections.push(`## 工具\n\n${tools.join("\n\n")}`);

  if (message.content.trim()) {
    sections.push(`## 回答\n\n${message.content.trim()}`);
  }
  return sections.join("\n\n");
}
