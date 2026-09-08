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
  for (const label of ["chat.copy", "chat.editQuestion", "chat.branch"]) {
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
  assert.match(messages, /"chat\.editSubmit": "保存并重新执行"/);
  assert.match(
    styles,
    /\.bubble\.user\.is-editing\s*\{[\s\S]*?width:\s*clamp\(280px, 48vw, 440px\);[\s\S]*?max-width:\s*min\(100%, calc\(100vw - 72px\)\);/,
  );
  assert.match(
    styles,
    /\.user-message-editor-input\s*\{[\s\S]*?min-height:\s*48px;[\s\S]*?max-height:\s*180px;[\s\S]*?border:\s*0\.5px solid/,
  );
  assert.match(source, /Math\.min\(textarea\.scrollHeight, 180\)/);
  assert.match(
    styles,
    /\.user-message-editor-btn\s*\{[\s\S]*?min-height:\s*28px;[\s\S]*?border:\s*0\.5px solid/,
  );
  assert.match(styles, /\.msg-row\.user:hover \.msg-actions/);
  assert.match(source, /className="assistant-message-footer"/);
  assert.match(source, /<MessageTokenStats/);
  assert.match(styles, /\.assistant-message-footer/);
  assert.match(
    styles,
    /\.bubble\.assistant \.assistant-message-footer \.msg-actions \{[\s\S]*?opacity: 1;[\s\S]*?background: transparent;/,
  );
  assert.match(styles, /\.msg-token-stats\[open\]/);
  assert.match(messages, /"chat\.tokenSummary": "共 \{total\} tokens"/);
  assert.match(messages, /"chat\.tokenInput": "输入 \{tokens\}"/);
  assert.doesNotMatch(styles, /\.user-message-edit-trigger/);
  assert.doesNotMatch(source, /MsgDissolveOverlay|onDeleteMessage/);
  assert.doesNotMatch(session, /const deleteMessage|persistAfterEditTruncate/);
  assert.doesNotMatch(styles, /msg-dissolve/);
});

test("assistant actions do not expose side-effecting regeneration", async () => {
  const [view, session, menu] = await Promise.all([
    readFile(chatViewUrl, "utf8"),
    readFile(chatSessionUrl, "utf8"),
    readFile(
      new URL(
        "../../components/chat/AssistantTurnContextMenu.tsx",
        import.meta.url,
      ),
      "utf8",
    ),
  ]);
  assert.doesNotMatch(view, /onRegenerate|chat\.regenerate/);
  assert.doesNotMatch(session, /regenerateMessage|retryLastAssistant/);
  assert.doesNotMatch(menu, /regenerate|RefreshCw/);
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

test("composer exposes an accessible hover-revealed input height control", async () => {
  const source = await readFile(chatViewUrl, "utf8");
  const messages = await readFile(messagesUrl, "utf8");
  const styles = await readFile(markdownCssUrl, "utf8");

  assert.match(source, /className="composer-expand-btn"/);
  assert.match(source, /className="composer-expand-indicator"/);
  assert.match(source, /d="M 22 11 A 11 11 0 0 1 33 22"/);
  assert.match(source, /className="composer-expand-glyph"/);
  assert.match(source, /aria-expanded=\{composerManuallyExpanded\}/);
  assert.match(source, /className="composer-expand-icon is-expand"/);
  assert.match(source, /className="composer-expand-icon is-collapse"/);
  assert.match(
    source,
    /className="composer-expand-icon is-expand"[\s\S]*?size=\{12\}/,
  );
  assert.match(
    source,
    /const capsuleComposerHasRichContent =\s*composerManuallyExpanded \|\|/,
  );
  assert.match(source, /Math\.min\(Math\.max\(el\.scrollHeight, 220\), 360\)/);
  assert.match(messages, /"chat\.composerExpand": "展开输入框"/);
  assert.match(messages, /"chat\.composerCollapse": "收起输入框"/);
  assert.match(styles, /\.composer-expand-indicator/);
  assert.match(
    styles,
    /\.composer-expand-btn:hover \.composer-expand-indicator,[\s\S]*?opacity:\s*0;[\s\S]*?stroke-dashoffset:\s*1;[\s\S]*?rotate\(28deg\) scale\(0\.68\);/,
  );
  assert.match(
    styles,
    /\.composer-expand-btn\[aria-expanded="false"\]:hover[\s\S]*?\.composer-expand-icon\.is-expand,[\s\S]*?opacity:\s*1;[\s\S]*?rotate\(0\) scale\(1\);/,
  );
  assert.match(
    styles,
    /\.composer\.is-input-expanded \.composer-input[\s\S]*?max-height:\s*min\(42vh, 360px\);/,
  );
  assert.match(
    styles,
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.composer-expand-btn/,
  );
  assert.match(
    styles,
    /\.composer-input\s*\{[\s\S]*?transition:[\s\S]*?height 220ms/,
  );
  assert.match(styles, /--composer-corner-radius:\s*22px/);
  assert.match(
    styles,
    /\.composer-expand-btn\s*\{[\s\S]*?width:\s*calc\(var\(--composer-corner-radius\) \* 2\);[\s\S]*?height:\s*calc\(var\(--composer-corner-radius\) \* 2\);/,
  );
  assert.match(
    styles,
    /\.composer-expand-btn::before\s*\{[\s\S]*?width:\s*17px;[\s\S]*?height:\s*17px;/,
  );
});

test("composer leaves ordinary clipboard text to the native textarea paste", async () => {
  const source = await readFile(chatViewUrl, "utf8");
  const nativeTextGate = source.indexOf(
    'if (text || clipboardTypes.includes("text/html")) return;',
  );
  const osClipboardFallback = source.indexOf(
    "const fromOs = await attachmentsFromOsClipboard();",
  );

  assert.ok(nativeTextGate >= 0, "missing native text paste gate");
  assert.ok(osClipboardFallback > nativeTextGate);
  assert.match(
    source,
    /if \(pathList\.length\)[\s\S]*?if \(created\.length\)[\s\S]*?insertClipboardText\(\);[\s\S]*?return;/,
  );
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
