import assert from "node:assert/strict";
import test from "node:test";
import { mergeToolCatalog } from "./agentToolCatalog.ts";

test("mergeToolCatalog preserves model and registered tool identities", () => {
  const tools = mergeToolCatalog(
    [
      { id: "browser", params: [] },
      { id: "image_gen", params: [] },
      { id: "workflow", params: [] },
    ],
    [
      {
        id: "browser",
        name: "astro_browser.open",
        namespace: "astro_browser",
        registeredName: "browser_open",
        description: "Browser tools",
        icon: "panel-top-open",
        params: [],
        tools: ["astro_browser.open", "astro_browser.snapshot"],
        functions: [
          {
            name: "astro_browser.open",
            namespace: "astro_browser",
            registeredName: "browser_open",
            description: "Open a URL",
            icon: "panel-top-open",
            params: [],
          },
        ],
      },
      {
        id: "workflow",
        name: "workflow.get_run",
        namespace: "workflow",
        registeredName: "workflow__get_run",
        description: "Smart workflows",
        icon: "workflow",
        exposure: "direct",
        params: [],
        tools: ["workflow.get_run", "workflow.generate_weekly_report"],
        functions: [
          {
            name: "workflow.generate_weekly_report",
            namespace: "workflow",
            registeredName: "workflow__123",
            description: "Generate a weekly report",
            icon: "workflow",
            exposure: "deferred",
            params: [{ name: "topic", type: "string", optional: false }],
          },
        ],
      },
      {
        id: "image_gen",
        name: "media.image_gen",
        namespace: "media",
        registeredName: "image_gen",
        description: "Generate images",
        icon: "palette",
        params: [],
        tools: ["media.image_gen"],
        functions: [
          {
            name: "media.image_gen",
            namespace: "media",
            registeredName: "image_gen",
            description: "Generate images",
            icon: "palette",
            params: [],
          },
        ],
      },
    ],
  );

  const browser = tools.find((tool) => tool.id === "browser");
  assert.equal(browser?.namespace, "astro_browser");
  assert.deepEqual(browser?.tools, ["astro_browser.open", "astro_browser.snapshot"]);
  assert.equal(browser?.functions?.[0]?.registeredName, "browser_open");

  const image = tools.find((tool) => tool.id === "image_gen");
  assert.equal(image?.namespace, "media");
  assert.equal(image?.functions?.[0]?.name, "media.image_gen");

  const workflow = tools.find((tool) => tool.id === "workflow");
  assert.equal(workflow?.namespace, "workflow");
  assert.equal(workflow?.functions?.[0]?.exposure, "deferred");
  assert.equal(workflow?.functions?.[0]?.params?.[0]?.name, "topic");
});
