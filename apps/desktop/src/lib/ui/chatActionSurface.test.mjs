import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const chatViewUrl = new URL(
  "../../components/chat/ChatView.tsx",
  import.meta.url,
);
const contextUsagePopoverUrl = new URL(
  "../../components/chat/ContextUsagePopover.tsx",
  import.meta.url,
);
const messagesUrl = new URL("../../i18n/messages.ts", import.meta.url);
const chatSessionUrl = new URL(
  "../../hooks/chat/useChatSession.ts",
  import.meta.url,
);
const coreCssUrl = new URL(
  "../../styles/features/chat/core.css",
  import.meta.url,
);
const markdownCssUrl = new URL(
  "../../styles/features/chat/markdown.css",
  import.meta.url,
);

test("hover actions exclude deletion and only the latest user question can be edited", async () => {
  const [source, session, messages, styles] = await Promise.all([
    readFile(chatViewUrl, "utf8"),
    readFile(chatSessionUrl, "utf8"),
    readFile(messagesUrl, "utf8"),
    readFile(coreCssUrl, "utf8"),
  ]);
  const messageActions = source.match(
    /export function MessageActions[\s\S]*?\n}\n\nfunction InlineUserMessageEditor/,
  )?.[0];

  assert.ok(messageActions, "missing message action surface");
  for (const label of [
    "chat.copy",
    "chat.editQuestion",
    "chat.regenerate",
    "chat.branch",
  ]) {
    assert.ok(messageActions.includes(`t("${label}")`), label);
  }
  assert.doesNotMatch(messageActions, /onDelete|chat\.delete|Trash2/);
  assert.match(source, /findLastUserEntryId\(messages\)/);
  assert.match(source, /m\.id === lastUserMessageId/);
  assert.match(source, /<InlineUserMessageEditor/);
  assert.match(source, /event\.key === "Escape"/);
  assert.match(source, /event\.metaKey \|\| event\.ctrlKey/);
  assert.match(session, /findLastUserEntryIndex\(messages\)/);
  assert.match(
    session,
    /await sendImmediate\(\{[\s\S]*?truncateTo: idx,[\s\S]*?reuseUserId: userMsg\.id/,
  );
  assert.match(messages, /"chat\.editSubmit": "保存并重新生成"/);
  assert.match(styles, /\.bubble\.user\.is-editing/);
  assert.match(styles, /\.msg-row\.user:hover \.msg-actions/);
  assert.doesNotMatch(styles, /\.user-message-edit-trigger/);
  assert.doesNotMatch(source, /MsgDissolveOverlay|onDeleteMessage/);
  assert.doesNotMatch(session, /const deleteMessage|persistAfterEditTruncate/);
  assert.doesNotMatch(styles, /msg-dissolve/);
});

test("chat surface exposes realtime voice without restoring legacy ASR or read-aloud controls", async () => {
  const source = await readFile(chatViewUrl, "utf8");
  const messages = await readFile(messagesUrl, "utf8");
  const realtime = await readFile(
    new URL("../../hooks/chat/useRealtimeConversation.ts", import.meta.url),
    "utf8",
  );
  const styles = await Promise.all([
    readFile(coreCssUrl, "utf8"),
    readFile(markdownCssUrl, "utf8"),
  ]);

  for (const removedContract of [
    "tts_synthesize",
    "speech_to_text",
    "MediaRecorder",
    "chat.readAloud",
  ]) {
    assert.doesNotMatch(source, new RegExp(removedContract), removedContract);
    assert.doesNotMatch(messages, new RegExp(removedContract), removedContract);
  }

  assert.match(source, /useRealtimeConversation/);
  assert.match(source, /composer-realtime-btn/);
  assert.match(realtime, /AudioWorkletNode/);
  assert.match(realtime, /send_realtime_audio/);
  assert.match(realtime, /MAX_QUEUED_AUDIO_FRAMES/);
  assert.match(realtime, /new RTCPeerConnection/);
  assert.match(realtime, /createDataChannel\("oai-events"\)/);
  assert.match(realtime, /transport = "webrtc"/);
  assert.match(realtime, /transport,\n\s+sdp: offerSdp/);
  assert.match(realtime, /transport !== "existing_call"/);
  assert.match(realtime, /else if \(transport === "websocket"\)/);
  assert.match(realtime, /input_audio_speech_started/);
  assert.match(messages, /"chat\.realtimeStart": "开始实时语音"/);
  assert.match(messages, /"chat\.realtimeUnavailable"/);
  assert.match(styles.join("\n"), /realtime-mic-pulse/);
  assert.doesNotMatch(styles.join("\n"), /msg-tts-spin|is-recording/);
});

test("composer does not expose a reasoning control", async () => {
  const source = await readFile(chatViewUrl, "utf8");
  const app = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");
  const palette = await readFile(
    new URL("../../components/chat/ComposerPalette.tsx", import.meta.url),
    "utf8",
  );
  const styles = await readFile(markdownCssUrl, "utf8");

  for (const removedContract of ["composer-thinking-btn", "<Lightbulb"]) {
    assert.doesNotMatch(source, new RegExp(removedContract), removedContract);
  }
  assert.doesNotMatch(
    palette,
    /PaletteKind = "slash" \| "mention" \| "thinking"/,
  );
  assert.doesNotMatch(styles, /\.composer-thinking-btn/);
  assert.match(app, /thinking:\s*thinkingPrefs\.level/);
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
  assert.match(
    source,
    /invoke<PermissionSettings>\(\s*"get_permission_settings"\s*,?\s*\)/,
  );
  assert.match(source, /"set_permission_preset"/);
  assert.match(source, /data-approval-mode=\{mode\}/);
  assert.match(source, /chat\.approval\.fullAccessConfirmTitle/);
  assert.match(messages, /"chat\.approval\.askForApproval": "请求批准"/);
  assert.match(messages, /"chat\.approval\.approveForMe": "帮我批准"/);
  assert.match(messages, /"chat\.approval\.fullAccess": "完全访问"/);
  assert.match(styles, /\.composer-policy-menu/);
  assert.match(styles, /\.composer-policy-pill\.is-full-access/);
  assert.match(source, /className="composer-mode-pill-label"/);
  assert.match(source, /className="composer-policy-current-icon"/);
  assert.match(source, /className="composer-mode-chevron"/);
  assert.match(styles, /container-name:\s*chat-composer/);
  assert.match(
    styles,
    /@container chat-composer \(max-width: 640px\)[\s\S]*?\.composer-mode-pill-label,[\s\S]*?\.composer-policy-approval,[\s\S]*?\.composer-mode-chevron\s*\{\s*display:\s*none;/,
  );
  assert.match(
    styles,
    /@container chat-composer \(max-width: 640px\)[\s\S]*?\.composer-policy-current-icon\s*\{\s*display:\s*block;/,
  );
});

test("composer keeps context usage available at every occupancy level", async () => {
  const source = await readFile(chatViewUrl, "utf8");
  const styles = await readFile(markdownCssUrl, "utf8");

  assert.match(source, /className="composer-context-wrap"/);
  assert.match(source, /<ContextUsagePopover/);
  assert.doesNotMatch(source, /showContextControl/);
  assert.doesNotMatch(source, /<ChartPie/);
  assert.match(source, /className="composer-context-ring-value"/);
  assert.match(source, /strokeDasharray=\{`\$\{contextProgress\} 100`\}/);
  assert.match(styles, /\.composer-context-ring-track/);
  assert.match(styles, /\.composer-context-btn\.is-warning/);
  assert.match(styles, /\.composer-context-btn\.is-critical/);
});

test("context usage preview opens on hover and stays open across the portal gap", async () => {
  const source = await readFile(chatViewUrl, "utf8");
  const popover = await readFile(contextUsagePopoverUrl, "utf8");

  assert.match(source, /onPointerEnter=\{openContextPopover\}/);
  assert.match(source, /onPointerLeave=\{scheduleContextPopoverClose\}/);
  assert.match(source, /window\.setTimeout\([\s\S]*?, 160\)/);
  assert.match(source, /onPointerEnter=\{cancelContextPopoverClose\}/);
  assert.match(popover, /onPointerEnter=\{onPointerEnter\}/);
  assert.match(popover, /onPointerLeave=\{onPointerLeave\}/);
});
