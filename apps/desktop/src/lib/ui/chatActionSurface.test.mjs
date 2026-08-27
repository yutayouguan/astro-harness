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
