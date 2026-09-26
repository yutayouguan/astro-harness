import { readFile } from "node:fs/promises";

/**
 * 聊天渲染源码面：消息行（含消息操作/用量/工具/引用渲染）已从 ChatView 拆到
 * ChatMessageRow.tsx。断言“存在某段渲染标记”的用例读合并源码即可；
 * 断言“ChatView 里不应出现”的用例仍单独读 ChatView.tsx。
 */
export const chatViewUrl = new URL(
  "../../components/chat/ChatView.tsx",
  import.meta.url,
);
export const chatMessageRowUrl = new URL(
  "../../components/chat/ChatMessageRow.tsx",
  import.meta.url,
);

export async function readChatRenderSource() {
  const [view, row] = await Promise.all([
    readFile(chatViewUrl, "utf8"),
    readFile(chatMessageRowUrl, "utf8"),
  ]);
  return `${view}\n${row}`;
}
