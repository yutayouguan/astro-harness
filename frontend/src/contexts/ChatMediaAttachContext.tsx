/** 聊天区：将媒体路径引用为输入框附件（供消息内图片工具栏使用）。 */
import { createContext, useContext, type ReactNode } from "react";

export type ChatMediaAttachApi = {
  /** 把本地媒体路径挂到当前输入附件，并聚焦输入框 */
  attachMediaPath: (path: string) => Promise<void>;
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
