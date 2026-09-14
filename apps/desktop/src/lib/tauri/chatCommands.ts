import { invoke } from "@tauri-apps/api/core";
import type { ChatAttachmentKind, ReasoningEffort } from "../../types.ts";
import type { ChatInteractionMode } from "../chat/chatMode.ts";

export type InstalledSkillSummary = {
  id: string;
  name: string;
  enabled?: boolean;
};

export type McpServerSummary = {
  id: string;
  name: string;
  enabled: boolean;
  [key: string]: unknown;
};

export type StartChatAttachment = {
  name: string;
  mime: string;
  kind: ChatAttachmentKind;
  size: number;
  dataBase64: string | null;
  localPath: string | null;
};

export type StartChatRequest = {
  content: string;
  provider: string;
  providerId: string;
  model: string;
  sessionId: string;
  useMemory: boolean;
  thinkingEnabled: boolean;
  reasoningEffort: ReasoningEffort;
  resumeJson?: string;
  keepChatBubbles?: number;
  interactionMode: ChatInteractionMode;
  projectId: string;
  attachments: StartChatAttachment[];
};

export type ThreadAttachment = {
  id: string;
  threadId: string;
  attachmentType: string;
  identityKey: string;
  payload: unknown;
  createdAt: number;
};

export type ThreadAttachmentPage = {
  data: ThreadAttachment[];
  nextCursor: string | null;
};

type InvokeFn = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export function createChatCommands(invokeFn: InvokeFn = invoke) {
  return {
    listInstalledSkills: () =>
      invokeFn<InstalledSkillSummary[]>("list_installed_skills"),
    getMcpServers: () => invokeFn<McpServerSummary[]>("get_mcp_servers"),
    setMcpServers: (servers: McpServerSummary[]) =>
      invokeFn<void>("set_mcp_servers", { servers }),
    saveUpload: (args: {
      sessionId: string;
      fileName: string;
      dataBase64: string;
      messageId: string;
    }) => invokeFn<{ path: string }>("save_chat_upload", args),
    addThreadAttachment: (args: {
      threadId: string;
      attachmentType: string;
      identityKey: string;
      payload: unknown;
    }) =>
      invokeFn<{ outcome: "created" | "existing"; attachment: ThreadAttachment }>(
        "add_thread_attachment",
        args,
      ),
    listThreadAttachments: (args: {
      threadId: string;
      cursor?: string | null;
      limit?: number;
    }) =>
      invokeFn<ThreadAttachmentPage>("list_thread_attachments", args),
    removeThreadAttachment: (args: {
      threadId: string;
      attachmentType: string;
      identityKey: string;
    }) => invokeFn<boolean>("remove_thread_attachment", args),
    start: (request: StartChatRequest) =>
      invokeFn<string>("start_chat", { request }),
  };
}

export const chatCommands = createChatCommands();
