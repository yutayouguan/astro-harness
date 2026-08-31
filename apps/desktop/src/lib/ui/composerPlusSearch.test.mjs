import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const styles = await readFile(
  new URL("../../styles/features/chat/markdown.css", import.meta.url),
  "utf8",
);
const component = await readFile(
  new URL("../../components/chat/ComposerPlusMenu.tsx", import.meta.url),
  "utf8",
);

test("plus-menu search icon and input share one inset surface", () => {
  assert.match(component, /composer-mcp-menu-search composer-plus-search/);
  assert.match(component, /<Search[\s\S]+?<input/);
  assert.match(
    styles,
    /\.composer-mcp-menu-search\.composer-plus-search\s*\{[\s\S]*?display:\s*flex;[\s\S]*?align-items:\s*center;[\s\S]*?min-height:\s*38px;[\s\S]*?border-radius:\s*12px;[\s\S]*?background:/,
  );
  assert.match(
    styles,
    /\.composer-mcp-menu-search\.composer-plus-search input\s*\{[\s\S]*?border:\s*0;[\s\S]*?background:\s*transparent;/,
  );
});

test("plus-menu search exposes a restrained theme focus ring", () => {
  assert.match(
    styles,
    /\.composer-mcp-menu-search\.composer-plus-search:focus-within\s*\{[\s\S]*?var\(--tone, var\(--accent\)\)[\s\S]*?0 0 0 3px/,
  );
});

test("MCP settings footer uses compact text without shrinking its hit area", () => {
  assert.match(
    styles,
    /\.composer-mcp-menu-footer\s*\{[\s\S]*?padding:\s*10px 12px;[\s\S]*?font-size:\s*12px;/,
  );
});
