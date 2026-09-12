import assert from "node:assert/strict";
import test from "node:test";
import { createFixture, fixtureOutput, responseEvents } from "./first-meeting-fixture.mjs";
import { once } from "node:events";

const body = text => ({ tools: [{ type: "function", name: "request_user_input_async" }, { type: "custom", name: "apply_patch" }], input: [{ role: "user", content: [{ type: "input_text", text }] }] });
const workspace = "/isolated-qa/workspace";
test("runtime context after a user request does not hide the fixture intent", () => {
  const request = body("Please follow this skill (first-meeting): 确认记住");
  request.input.push({ role: "user", content: [{ type: "input_text", text: "<runtime_context>Current turn metadata</runtime_context>" }] });
  assert.ok(fixtureOutput(request, workspace).some(item => item.name === "request_user_input_async"));
});
test("unrelated later input cannot reuse an earlier confirmation", () => {
  const request = body("确认记住");
  request.input.push({ role: "assistant", content: [{ type: "output_text", text: "Previous answer" }] });
  request.input.push({ role: "user", content: [{ type: "input_text", text: "现在换个话题" }] });
  assert.ok(fixtureOutput(request, workspace).every(item => item.type !== "custom_tool_call"));
});
test("first meeting and rejected summaries never emit file writes", () => {
  for (const input of ["Please follow this skill (first-meeting): 确认记住", "叫我搭档", "暂不保存", "先做任务"]) {
    assert.ok(fixtureOutput(body(input), workspace).every(item => item.type !== "custom_tool_call"));
  }
});
test("explicit fixture confirmation emits one bounded patch, then terminates", () => {
  const request = body("确认记住");
  const output = fixtureOutput(request, workspace);
  assert.equal(output.length, 1);
  assert.equal(output[0].name, "apply_patch");
  assert.ok(output[0].input.includes("/isolated-qa/workspace/USER.md"));
  assert.ok(!output[0].input.includes("SOUL.md"));
  request.input.push(...output, { type: "custom_tool_call_output", output: "done" });
  assert.ok(fixtureOutput(request, workspace).every(item => item.type === "message"));
});
test("fixture respects tool availability and emits complete native Responses items", () => {
  const request = body("确认记住");
  request.tools.pop();
  assert.ok(fixtureOutput(request, workspace).every(item => item.type === "message"));
  const output = fixtureOutput(body("叫我搭档"), workspace);
  const events = responseEvents(output);
  assert.deepEqual(events.at(-1).response.output, output);
  assert.equal(events.filter(event => event.type === "response.output_item.done").length, output.length);
});

test("loopback HTTP fixture supplies SSE and rejects non-fixture routes", async t => {
  const server = createFixture(workspace);
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => new Promise(resolve => server.close(resolve)));
  const base = `http://127.0.0.1:${server.address().port}`;
  assert.equal((await fetch(`${base}/outside`)).status, 403);
  const models = await fetch(`${base}/v1/models`).then(response => response.json());
  assert.deepEqual(models.data, [{ id: "qa-small" }]);
  const response = await fetch(`${base}/v1/responses`, { method: "POST", body: JSON.stringify({ ...body("叫我搭档"), stream: true }) });
  assert.equal(response.headers.get("content-type"), "text/event-stream");
  const events = (await response.text()).split("\n").filter(line => line.startsWith("data: {")).map(line => JSON.parse(line.slice(6)));
  assert.equal(events.at(-1).type, "response.completed");
  const question = events.at(-1).response.output.find(item => item.name === "request_user_input_async");
  assert.deepEqual(JSON.parse(question.arguments).questions[0].options, ["确认记住", "修改一下", "暂不保存"]);
});
