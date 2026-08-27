import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const chatViewUrl = new URL(
  "../../components/chat/ChatView.tsx",
  import.meta.url,
);
const messagesUrl = new URL("../../i18n/messages.ts", import.meta.url);
const coreCssUrl = new URL(
  "../../styles/features/chat/core.css",
  import.meta.url,
);
const markdownCssUrl = new URL(
  "../../styles/features/chat/markdown.css",
  import.meta.url,
);

test("message actions keep the supported conversation controls", async () => {
  const source = await readFile(chatViewUrl, "utf8");

  for (const label of [
    "chat.copy",
    "chat.regenerate",
    "chat.editResend",
    "chat.delete",
    "chat.branch",
  ]) {
    assert.ok(source.includes(`t("${label}")`), label);
  }
});

test("chat surfaces do not expose read-aloud or voice-input controls", async () => {
  const source = await readFile(chatViewUrl, "utf8");
  const messages = await readFile(messagesUrl, "utf8");
  const styles = await Promise.all([
    readFile(coreCssUrl, "utf8"),
    readFile(markdownCssUrl, "utf8"),
  ]);

  for (const removedContract of [
    "tts_synthesize",
    "speech_to_text",
    "MediaRecorder",
    "chat.readAloud",
    "chat.micStart",
    "chat.micStop",
    "chat.micTranscribing",
  ]) {
    assert.doesNotMatch(source, new RegExp(removedContract), removedContract);
    assert.doesNotMatch(messages, new RegExp(removedContract), removedContract);
  }

  assert.doesNotMatch(styles.join("\n"), /msg-tts-spin|mic-pulse|is-recording/);
});

test("composer approval selector keeps the three supported permission choices", async () => {
  const source = await readFile(chatViewUrl, "utf8");
  const messages = await readFile(messagesUrl, "utf8");
  const styles = await readFile(markdownCssUrl, "utf8");
  const presets = source.match(
    /const PERMISSION_PRESETS:[\s\S]*?= \[(?<items>[\s\S]*?)\];/,
  )?.groups?.items;

  assert.ok(presets, "missing approval preset contract");
  for (const preset of ["ask_for_approval", "approve_for_me", "full_access"]) {
    assert.match(presets, new RegExp(`"${preset}"`));
  }
  assert.doesNotMatch(presets, /read_only/);
  assert.match(source, /invoke<PermissionSettings>\("get_permission_settings"\)/);
  assert.match(source, /"set_permission_preset"/);
  assert.match(source, /data-approval-mode=\{mode\}/);
  assert.match(source, /chat\.approval\.fullAccessConfirmTitle/);
  assert.match(messages, /"chat\.approval\.askForApproval": "请求批准"/);
  assert.match(messages, /"chat\.approval\.approveForMe": "帮我批准"/);
  assert.match(messages, /"chat\.approval\.fullAccess": "完全访问"/);
  assert.match(styles, /\.composer-policy-menu/);
  assert.match(styles, /\.composer-policy-pill\.is-full-access/);
});

test("composer keeps context usage available at every occupancy level", async () => {
  const source = await readFile(chatViewUrl, "utf8");

  assert.match(source, /className="composer-context-wrap"/);
  assert.match(source, /<ContextUsagePopover/);
  assert.doesNotMatch(source, /showContextControl/);
});
