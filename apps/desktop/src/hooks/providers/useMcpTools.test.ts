import assert from "node:assert/strict";
import test from "node:test";
import {
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
