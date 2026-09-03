import assert from "node:assert/strict";
import test from "node:test";
import { mergeToolCatalog } from "./agentToolCatalog.ts";

test("mergeToolCatalog preserves model and registered tool identities", () => {
  const tools = mergeToolCatalog(
    [
      { id: "browser", params: [] },
      { id: "image_gen", params: [] },
    ],
    [
      {
        id: "browser",
        name: "browser.open",
        namespace: "browser",
        registeredName: "browser_open",
        description: "Browser tools",
        icon: "panel-top-open",
        params: [],
        tools: ["browser.open", "browser.snapshot"],
        functions: [
          {
            name: "browser.open",
            namespace: "browser",
            registeredName: "browser_open",
            description: "Open a URL",
            icon: "panel-top-open",
            params: [],
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
  assert.equal(browser?.namespace, "browser");
  assert.deepEqual(browser?.tools, ["browser.open", "browser.snapshot"]);
  assert.equal(browser?.functions?.[0]?.registeredName, "browser_open");

  const image = tools.find((tool) => tool.id === "image_gen");
  assert.equal(image?.namespace, "media");
  assert.equal(image?.functions?.[0]?.name, "media.image_gen");
});
