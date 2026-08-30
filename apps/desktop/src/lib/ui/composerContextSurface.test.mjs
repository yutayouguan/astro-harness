import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const chatViewUrl = new URL(
  "../../components/chat/ChatView.tsx",
  import.meta.url,
);
const previewUrl = new URL(
  "../../components/chat/ComposerContextPreview.tsx",
  import.meta.url,
);
const plusMenuUrl = new URL(
  "../../components/chat/ComposerPlusMenu.tsx",
  import.meta.url,
);
const stylesUrl = new URL(
  "../../styles/features/chat/markdown.css",
  import.meta.url,
);

test("selected attachments and tools render as interactive composer tokens", async () => {
  const source = await readFile(chatViewUrl, "utf8");

  assert.match(source, /composer-context-strip/);
  assert.match(source, /composer-preview-open/);
  assert.match(source, /composer-context-token-open/);
  assert.match(source, /setPreviewTarget\(\{ type: "attachment"/);
  assert.match(source, /setPreviewTarget\(\{ type: "context"/);
  assert.match(source, /serializeComposerContext\(composerContexts/);
});

test("quoted code and documents become file context tokens instead of image prompts", async () => {
  const [source, toolbar] = await Promise.all([
    readFile(chatViewUrl, "utf8"),
    readFile(new URL("../../components/media/MediaToolbar.tsx", import.meta.url), "utf8"),
  ]);

  assert.match(toolbar, /attachMediaPath\(path, kind\)/);
  assert.match(source, /kind === "code" \|\| kind === "html" \|\| kind === "document"/);
  assert.match(source, /createFileComposerContextToken\(path, description\)/);
  assert.match(source, /media\.quoteCodePrompt/);
  assert.match(source, /file: t\("chat\.contextToken\.file"\)/);
});

test("skills and MCP servers can be added to the composer and previewed", async () => {
  const [plusMenu, preview] = await Promise.all([
    readFile(plusMenuUrl, "utf8"),
    readFile(previewUrl, "utf8"),
  ]);

  assert.match(plusMenu, /onSelectSkill: \(skill: InstalledSkill\)/);
  assert.match(plusMenu, /onSelectMcp: \(server: McpServer\)/);
  assert.match(preview, /role="dialog"/);
  assert.match(preview, /invoke<SkillContent>\("get_skill_content"/);
});

test("context preview honors reduced motion and transparency", async () => {
  const styles = await readFile(stylesUrl, "utf8");

  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.composer-context-preview-dialog/);
  assert.match(styles, /@media \(prefers-reduced-transparency: reduce\)[\s\S]*?\.composer-context-preview-dialog/);
});
