import assert from "node:assert/strict";
import test from "node:test";
import {
  isMcpToolEnabled,
  LegacySseTransportError,
  normalizeMcpRuntimeStatus,
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

test("preserves Codex environment credential references", () => {
  const servers = parseMcpJson(
    JSON.stringify({
      mcpServers: {
        local: { command: "npx", env_vars: ["LOCAL_TOKEN"] },
        remote: {
          url: "https://example.com/mcp",
          bearer_token_env_var: "MCP_ACCESS_TOKEN",
          http_headers: { "X-Region": "us-east-1" },
          env_http_headers: { "X-API-Key": "MCP_API_KEY" },
        },
      },
    }),
  );

  assert.deepEqual(servers[0]?.envVars, ["LOCAL_TOKEN"]);
  assert.equal(servers[1]?.bearerTokenEnvVar, "MCP_ACCESS_TOKEN");
  assert.deepEqual(servers[1]?.headers, { "X-Region": "us-east-1" });
  assert.deepEqual(servers[1]?.envHttpHeaders, { "X-API-Key": "MCP_API_KEY" });
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

test("preserves Codex server and per-tool approval modes", () => {
  const [server] = parseMcpJson(
    JSON.stringify({
      mcpServers: {
        docs: {
          command: "npx",
          default_tools_approval_mode: "writes",
          tools: {
            read: { approval_mode: "approve" },
            publish: { enabled: false, approval_mode: "prompt" },
            legacy: true,
          },
        },
      },
    }),
  );

  assert.equal(server.defaultToolsApprovalMode, "writes");
  assert.equal(server.toolApprovalModes.read, "approve");
  assert.equal(server.toolApprovalModes.publish, "prompt");
  assert.equal(isMcpToolEnabled(server, "publish"), false);
  assert.equal(isMcpToolEnabled(server, "legacy"), true);
});

test("normalizes MCP runtime status and rejects unknown state strings", () => {
  assert.deepEqual(
    normalizeMcpRuntimeStatus({
      id: "docs",
      name: "Docs",
      status: "connected",
      tools: ["search", "read"],
      required: true,
      error: "",
      retryable: false,
      retryAttempt: 0,
    }),
    {
      id: "docs",
      name: "Docs",
      status: "connected",
      tools: ["search", "read"],
      required: true,
      error: undefined,
      retryable: false,
      retryAttempt: 0,
      nextRetryAtUnixMs: undefined,
      oauthAvailable: false,
      authenticated: false,
    },
  );
  assert.equal(
    normalizeMcpRuntimeStatus({ id: "future", status: "future_state" as never }).status,
    "unknown",
  );
});

test("preserves OAuth auth mode and auth-required runtime state", () => {
  const [server] = parseMcpJson(
    JSON.stringify({ url: "https://example.com/mcp", auth: "oauth" }),
  );
  assert.equal(server.auth, "oauth");
  const status = normalizeMcpRuntimeStatus({
    id: "remote",
    status: "auth-required",
    oauth_available: true,
  } as never);
  assert.equal(status.status, "auth-required");
  assert.equal(status.oauthAvailable, true);
  assert.equal(status.authenticated, false);
});

test("normalizes snake_case retry metadata", () => {
  const status = normalizeMcpRuntimeStatus({
    id: "docs",
    status: "backoff",
    retryable: true,
    retry_attempt: 3,
    next_retry_at_unix_ms: 1_700_000_000_000,
  } as never);

  assert.equal(status.status, "backoff");
  assert.equal(status.retryAttempt, 3);
  assert.equal(status.nextRetryAtUnixMs, 1_700_000_000_000);
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
