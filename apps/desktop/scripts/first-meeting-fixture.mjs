// Scripted Responses server for native acceptance. No credentials, model inference,
// outbound requests or file writes. Only a bound, marked QA workspace can be edited
// by the app through the emitted apply_patch call after an explicit test answer.
import http from "node:http";
import { randomUUID } from "node:crypto";
import { pathToFileURL } from "node:url";
import { join } from "node:path";
import { loadNativeManifest } from "../../../tools/lib/native-config-acceptance.mjs";

const textItem = text => ({ type: "message", id: `msg_${randomUUID()}`, role: "assistant", status: "completed", content: [{ type: "output_text", text, annotations: [] }] });
const textOf = item => typeof item.content === "string" ? item.content : (item.content ?? []).map(part => part.text ?? "").join("\n");

export function fixtureOutput(body, workspace) {
  const input = Array.isArray(body.input) ? body.input : [];
  const lastUser = input.findLastIndex(item => item.role === "user");
  const user = lastUser < 0 ? "" : textOf(input[lastUser]);
  const subsequent = input.slice(lastUser + 1);
  if (subsequent.some(item => ["function_call", "custom_tool_call"].includes(item.type))) {
    return [textItem("本机模拟服务：这一轮已结束。请按页面选项继续；档案写入结果需实际读回核对。")];
  }
  const tools = (body.tools ?? []).flatMap(tool => tool.tools ?? [tool]);
  const ask = tools.find(tool => tool.name === "request_user_input_async");
  const question = (title, options) => [{ type: "function_call", id: `fc_${randomUUID()}`, call_id: `call_${randomUUID()}`, name: "request_user_input_async", arguments: JSON.stringify({ questions: [{ title, options }] }), status: "completed" }];
  if (!ask || !user) return [textItem("Local QA only")];
  if (user.includes("Please follow this skill (first-meeting)")) {
    return [textItem("你好，我是 Astro。这里是本机模拟的初次见面，用来验证真实 APP 的交互，不代表真实模型推理。"), ...question("怎么称呼你比较舒服？", ["直接称呼我‘你’（推荐）", "叫我搭档", "先做任务"])];
  }
  if (user.includes("暂不保存") || user.includes("先做任务")) {
    return [textItem("好的，不写入档案。现在想一起完成什么任务？")];
  }
  if (user.includes("确认记住")) {
    const patchTool = tools.find(tool => tool.name === "apply_patch");
    if (!patchTool) return [textItem("测试失败：本轮未提供 apply_patch，未尝试写入。")];
    const patch = `*** Begin Patch\n*** Update File: ${join(workspace, "USER.md")}\n@@\n-- **What to call them:**\n+- **What to call them:** 搭档\n*** End Patch`;
    return [{ type: "custom_tool_call", id: `ct_${randomUUID()}`, call_id: `call_${randomUUID()}`, name: "apply_patch", input: patch, status: "completed" }];
  }
  if (user.includes("搭档")) {
    return [textItem("我准备只把 USER.md 中的称呼记为‘搭档’。姓名、背景保持空白，IDENTITY.md 和 SOUL.md 保持原样。"), ...question("要记住这项称呼吗？", ["确认记住", "修改一下", "暂不保存"])];
  }
  return [textItem("本机模拟服务：没有对应的测试回答，不会写入文件。")];
}

export function responseEvents(output) {
  const id = `resp_${randomUUID()}`;
  const response = { id, object: "response", created_at: Math.floor(Date.now() / 1000), status: "completed", output, usage: { input_tokens: 10, output_tokens: 10, total_tokens: 20 } };
  const events = [{ type: "response.created", response: { ...response, status: "in_progress", output: [] } }];
  output.forEach((item, index) => {
    const initial = { ...item, status: "in_progress" };
    if (item.type === "function_call") initial.arguments = "";
    if (item.type === "custom_tool_call") initial.input = "";
    if (item.type === "message") initial.content = [];
    events.push({ type: "response.output_item.added", output_index: index, item: initial });
    if (item.type === "message") events.push({ type: "response.output_text.delta", output_index: index, content_index: 0, item_id: item.id, delta: item.content[0].text });
    events.push({ type: "response.output_item.done", output_index: index, item });
  });
  events.push({ type: "response.completed", response });
  return events;
}

export function createFixture(workspace) {
  const server = http.createServer(async (request, response) => {
    const reply = (status, data) => { response.writeHead(status, { "Content-Type": "application/json" }); response.end(JSON.stringify(data)); };
    if (request.url === "/v1/models" && request.method === "GET") { reply(200, { data: [{ id: "qa-small" }] }); return; }
    if (request.url !== "/v1/responses" || request.method !== "POST") { reply(403, { error: { message: "Only local QA routes are allowed" } }); return; }
    try {
      let raw = "";
      request.setEncoding("utf8");
      for await (const chunk of request) {
        raw += chunk.toString();
        if (Buffer.byteLength(raw) > 2_000_000) { reply(413, { error: { message: "request too large" } }); return; }
      }
      const body = JSON.parse(raw);
      const output = fixtureOutput(body, workspace);
      // Never log headers, credentials, prompts or tool arguments.
      console.log(JSON.stringify({ route: request.url, stream: !!body.stream, output: output.map(item => item.name ?? item.type) }));
      const events = responseEvents(output);
      if (!body.stream) { reply(200, events.at(-1).response); return; }
      response.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-cache" });
      events.forEach((event, sequence_number) => response.write(`event: ${event.type}\ndata: ${JSON.stringify({ ...event, sequence_number })}\n\n`));
      response.end("data: [DONE]\n\n");
    } catch { reply(400, { error: { message: "invalid QA request" } }); }
  });
  server.on("connect", (_request, socket) => socket.end("HTTP/1.1 403 Forbidden\r\n\r\n"));
  return server;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv.length !== 3) throw new Error("Pass the isolated native manifest.json; never a user workspace");
  const manifest = await loadNativeManifest(process.argv[2]);
  const server = createFixture(join(manifest.astroRoot, "workspace"));
  server.listen(0, "127.0.0.1", () => console.log(`QA_PROVIDER_URL=http://127.0.0.1:${server.address().port}/v1`));
}
