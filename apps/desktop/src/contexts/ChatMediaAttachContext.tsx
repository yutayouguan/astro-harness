/** 聊天区：将媒体或文件路径引用到输入框（供消息内文件工具栏使用）。 */
import { createContext, useContext, type ReactNode } from "react";
import type { MediaActionKind } from "../lib/media/mediaActions";

export type ChatMediaAttachApi = {
  /** 把路径挂到当前输入附件或文件上下文，并聚焦输入框 */
  attachMediaPath: (path: string, kind: MediaActionKind) => Promise<void>;
};

const ChatMediaAttachContext = createContext<ChatMediaAttachApi | null>(null);

export function ChatMediaAttachProvider({
  value,
  children,
}: {
  value: ChatMediaAttachApi;
  children: ReactNode;
}) {
  return (
    <ChatMediaAttachContext.Provider value={value}>
      {children}
    </ChatMediaAttachContext.Provider>
  );
}

export function useChatMediaAttach(): ChatMediaAttachApi | null {
  return useContext(ChatMediaAttachContext);
}
