import assert from "node:assert/strict";
import test from "node:test";
import {
  isMcpToolEnabled,
  LegacySseTransportError,
  parseMcpJson,
} from "./useMcpTools.ts";

test("imports stdio and Streamable HTTP servers", () => {
  const servers = parseMcpJson(
    JSON.stringify({
      mcpServers: {
        local: { command: "npx", args: ["-y", "example-mcp"] },
        remote: { type: "streamableHttp", url: "https://example.com/mcp" },
      },
    }),
  );

  assert.deepEqual(
    servers.map((server) => server.type),
    ["stdio", "streamableHttp"],
  );
  assert.deepEqual(
    servers.map((server) => [server.startupTimeoutSecs, server.toolTimeoutSecs]),
    [[10, 60], [10, 60]],
  );
});

test("preserves Codex timeout field names", () => {
  const [server] = parseMcpJson(
    JSON.stringify({
      mcpServers: {
        local: { command: "npx", startup_timeout_sec: 17, tool_timeout_sec: 91 },
      },
    }),
  );

  assert.equal(server.startupTimeoutSecs, 17);
  assert.equal(server.toolTimeoutSecs, 91);
});

test("bounds imported timeout values", () => {
  const [server] = parseMcpJson(
    JSON.stringify({ command: "npx", startupTimeoutSecs: 0, toolTimeoutSecs: 9000 }),
  );

  assert.equal(server.startupTimeoutSecs, 1);
  assert.equal(server.toolTimeoutSecs, 3600);
});

test("preserves required, cwd and allow/deny policy", () => {
  const [server] = parseMcpJson(
    JSON.stringify({
      command: "npx",
      cwd: "packages/server",
      required: true,
      enabled_tools: ["read", "search"],
      disabled_tools: ["search"],
    }),
  );

  assert.equal(server.required, true);
  assert.equal(server.cwd, "packages/server");
  assert.deepEqual(server.enabledTools, ["read", "search"]);
  assert.deepEqual(server.disabledTools, ["search"]);
  assert.equal(isMcpToolEnabled(server, "read"), true);
  assert.equal(isMcpToolEnabled(server, "search"), false);
  assert.equal(isMcpToolEnabled(server, "unknown"), false);
});

test("rejects an explicit legacy SSE transport", () => {
  assert.throws(
    () =>
      parseMcpJson(
        JSON.stringify({
          mcpServers: {
            legacy: { type: "sse", url: "http://localhost:3000/sse" },
          },
        }),
      ),
    LegacySseTransportError,
  );
});

test("does not infer an /sse URL as Streamable HTTP", () => {
  assert.throws(
    () =>
      parseMcpJson(
        JSON.stringify({
          mcpServers: {
            legacy: { url: "http://localhost:3000/sse" },
          },
        }),
      ),
    LegacySseTransportError,
  );
});
