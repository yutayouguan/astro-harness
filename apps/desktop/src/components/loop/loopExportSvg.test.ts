import assert from "node:assert/strict";
import test from "node:test";
import { buildWorkflowSvg } from "./loopExportSvg.ts";

test("workflow SVG contains pure vector nodes and connections", () => {
  const svg = buildWorkflowSvg({
    title: "Knowledge & answer",
    summary: "2 nodes · 1 connection",
    dark: false,
    nodes: [
      {
        id: "input",
        x: 0,
        y: 0,
        width: 180,
        height: 58,
        label: "User <input>",
        subtitle: "Manual Trigger",
        color: "#60a5fa",
      },
      {
        id: "output",
        x: 300,
        y: 0,
        width: 180,
        height: 58,
        label: "Output",
        subtitle: "Result",
        color: "#fb923c",
      },
    ],
    edges: [{ source: "input", target: "output", type: "smoothstep" }],
  });

  assert.match(svg, /^<\?xml version="1\.0" encoding="UTF-8"\?>/);
  assert.match(svg, /class="edge"/);
  assert.match(svg, /M 228 87 H 348/);
  assert.match(svg, /User &lt;input&gt;/);
  assert.doesNotMatch(svg, /foreignObject|data:image\/png/);
});

test("workflow SVG drops edges whose endpoints are missing", () => {
  const svg = buildWorkflowSvg({
    title: "Empty edge",
    summary: "1 node · 0 connections",
    dark: true,
    nodes: [
      {
        id: "only",
        x: 0,
        y: 0,
        width: 180,
        height: 58,
        label: "Only",
        subtitle: "Node",
        color: "javascript:alert(1)",
      },
    ],
    edges: [{ source: "only", target: "missing" }],
  });

  assert.doesNotMatch(svg, /class="edge"/);
  assert.doesNotMatch(svg, /javascript:/);
  assert.match(svg, /fill="#8b5cf6"/);
});
