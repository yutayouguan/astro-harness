// Local-only, non-billing provider for manual native onboarding acceptance.
// Start with: node scripts/onboarding-provider-fixture.mjs
// POST /qa/offline or /qa/online toggles a recoverable outage.
import http from "node:http";

let offline = false;
const server = http.createServer((request, response) => {
  const reply = (status, body) => {
    response.writeHead(status, { "Content-Type": "application/json" });
    response.end(JSON.stringify(body));
  };
  if (request.method === "POST" && ["/qa/offline", "/qa/online"].includes(request.url)) {
    offline = request.url === "/qa/offline";
    reply(200, { offline });
    return;
  }
  if (!["/v1/models", "/v1/responses"].includes(request.url)) {
    reply(403, { error: { message: "QA blocks non-fixture traffic" } });
    return;
  }
  console.log(JSON.stringify({ method: request.method, route: request.url, offline }));
  if (offline) {
    reply(503, { error: { message: "network unavailable (local QA)" } });
  } else if (request.url === "/v1/models" && request.method === "GET") {
    reply(200, { data: [{ id: "qa-small" }, { id: "qa-reason" }] });
  } else if (request.url === "/v1/responses" && request.method === "POST") {
    reply(200, { id: "qa-response", object: "response", status: "completed",
      output: [{ type: "message", role: "assistant", content: [{ type: "output_text", text: "Local QA only" }] }] });
  } else {
    reply(405, { error: { message: "Method not allowed" } });
  }
});
server.on("connect", (_request, socket) => socket.end("HTTP/1.1 403 Forbidden\r\n\r\n"));
server.listen(0, "127.0.0.1", () => {
  console.log(`QA_PROVIDER_URL=http://127.0.0.1:${server.address().port}/v1`);
});
