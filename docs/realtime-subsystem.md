# Realtime subsystem

## Goals

Astro Realtime is a versioned conversation subsystem, not a special case in the
agent loop. It owns transport negotiation, provider wire decoding, typed events,
handoff routing, and durable transcript projection. The ordinary agent runtime
continues to own model turns and tools.

## Boundaries

```text
Desktop WebRTC media/data channel
             |
             | SDP offer/answer
             v
agent-server / Tauri bridge
             |
             v
agent-realtime
  transport: websocket | webrtc | existing_call
  protocol:  v2 OpenAI GA | v3 frameless/live
  events:    provider JSON -> RealtimeEvent
  handoff:   request -> agent turn -> context append/complete
             |
             +---- live EventMsg ----> UI
             |
             +---- RealtimeItem -----> rollout JSONL
```

`agent-realtime` depends only on `agent-protocol`. `agent-core` adapts typed
handoff requests to normal turns and persists the history projection. Provider
wire JSON never crosses the crate boundary.

## Transport contract

- `websocket` opens the provider Realtime endpoint and carries audio plus events.
- `webrtc` accepts a browser SDP offer, creates a call through
  `POST /v1/realtime/calls`, returns the answer SDP, and opens a server-side
  sideband WebSocket using the returned call id.
- `existing_call` attaches only the sideband WebSocket. It never mutates session
  configuration because the call owner already established it.
- V3 sideband reconnects with bounded exponential backoff. V2 closes on loss so
  callers do not accidentally replay non-idempotent audio or response requests.

## Version contract

- V2 is the public OpenAI GA default and uses `session.update`, conversation
  items, `response.create`, and the GA transcript/audio event names.
- V3 is explicit. It uses the frameless `/live/{call_id}` sideband, session
  context/delegation events, and BEM channel routing. It is not silently selected
  for public OpenAI models.

## Handoff and BEM

A typed `HandoffRequested` event starts or steers a normal Astro turn. The turn's
assistant deltas are streamed back to the active handoff. `thinking` sends an
unqualified context append, `commentary` selects the commentary channel, and
`bem_tags` parses `[ANALYSIS]`, `[COMMENTARY]`, and `[FINAL]` (plus configured
prefixes) before selecting commentary or speakable output. Completion is sent
exactly once after the turn reaches a terminal event.

## Durable history

Raw deltas remain transient. A `RealtimeHistory` reducer writes only:

- session started;
- complete user/assistant transcript segments;
- BEM item promotions that connect a regular turn item to a realtime session;
- session closed with `ended` or `failed` outcome.

This makes replay deterministic without storing audio or duplicating every
provider delta.

## Delivery phases

1. Versioned protocol types and parser tests.
2. `agent-realtime` transports, WebRTC call creation, sideband recovery, and BEM.
3. Core lifecycle, handoff bridge, rollout projection, and gRPC/Tauri contracts.
4. Browser WebRTC media path and typed-event UI.
5. Focused tests, integration checks, and dirty-worktree-safe commit.
