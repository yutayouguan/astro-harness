import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const sessionHookUrl = new URL(
  "../../hooks/chat/useChatSession.ts",
  import.meta.url,
);
const tauriLibUrl = new URL("../../../src-tauri/src/lib.rs", import.meta.url);
const sessionCommandUrl = new URL(
  "../../../src-tauri/src/commands/chat/session.rs",
  import.meta.url,
);
const rolloutUrl = new URL(
  "../../../../../crates/agent-rollout/src/reconstruction.rs",
  import.meta.url,
);

test("context usage is rehydrated from the persisted rollout when a session opens", async () => {
  const [hook, lib, command, rollout] = await Promise.all([
    readFile(sessionHookUrl, "utf8"),
    readFile(tauriLibUrl, "utf8"),
    readFile(sessionCommandUrl, "utf8"),
    readFile(rolloutUrl, "utf8"),
  ]);

  // 前端：打开会话时回读后端快照，只在比本地新时覆盖，并写回按会话缓存。
  assert.match(
    hook,
    /invoke<[\s\S]*?>\("get_context_usage", \{ sessionId: sid \}\)/,
  );
  assert.match(
    hook,
    /prev && prev\.updatedAt >= snapshot\.updatedAt \? prev : snapshot/,
  );
  assert.match(hook, /saveContextUsageForSession\(sid, snapshot\)/);
  assert.match(hook, /void hydrateContextUsage\(sid\)/);

  // 后端：命令注册 + rollout 里取最近一次上下文占用快照。
  assert.match(lib, /commands::session::get_context_usage/);
  assert.match(
    command,
    /pub async fn get_context_usage\([\s\S]*?\) -> Result<Option<agent_protocol::ContextUsageEvent>, String>/,
  );
  assert.match(
    rollout,
    /pub fn latest_context_usage\([\s\S]*?ContextUsage\(event\)[\s\S]*?Some\(event\.clone\(\)\)/,
  );
});
