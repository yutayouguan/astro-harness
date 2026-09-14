import assert from "node:assert/strict";
import test from "node:test";

import { createChatCommands } from "./chatCommands.ts";

test("chat command gateway preserves canonical Tauri command envelopes", async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const commands = createChatCommands(async (command, args) => {
    calls.push({ command, args });
    return [] as never;
  });

  await commands.listInstalledSkills();
  await commands.getMcpServers();
  await commands.setMcpServers([{ id: "mcp", name: "MCP", enabled: true }]);
  await commands.saveUpload({
    sessionId: "s1",
    fileName: "a.png",
    dataBase64: "AA==",
    messageId: "u1",
  });
  await commands.addThreadAttachment({
    threadId: "s1",
    attachmentType: "workspace_file",
    identityKey: "a.png",
    payload: { path: "a.png" },
  });
  await commands.listThreadAttachments({ threadId: "s1", limit: 20 });
  await commands.removeThreadAttachment({
    threadId: "s1",
    attachmentType: "workspace_file",
    identityKey: "a.png",
  });
  await commands.start({
    content: "hello",
    provider: "openai",
    providerId: "p1",
    model: "gpt-test",
    sessionId: "s1",
    useMemory: true,
    thinkingEnabled: false,
    reasoningEffort: "high",
    interactionMode: "agent",
    projectId: "default",
    attachments: [],
  });

  assert.deepEqual(
    calls.map((call) => call.command),
    [
      "list_installed_skills",
      "get_mcp_servers",
      "set_mcp_servers",
      "save_chat_upload",
      "add_thread_attachment",
      "list_thread_attachments",
      "remove_thread_attachment",
      "start_chat",
    ],
  );
  assert.deepEqual(calls[7]?.args, {
    request: {
      content: "hello",
      provider: "openai",
      providerId: "p1",
      model: "gpt-test",
      sessionId: "s1",
      useMemory: true,
      thinkingEnabled: false,
      reasoningEffort: "high",
      interactionMode: "agent",
      projectId: "default",
      attachments: [],
    },
  });
});
