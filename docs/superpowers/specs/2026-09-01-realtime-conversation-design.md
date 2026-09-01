# Realtime Conversation Subsystem

## Scope

Astro exposes a thread-scoped realtime voice session backed by the OpenAI Realtime GA WebSocket
protocol. The desktop never receives the long-lived provider API key: Tauri resolves the selected
provider, forwards credentials to the local gRPC server, and the Rust runtime owns the WebSocket.

This implementation deliberately uses the server-side WebSocket transport. Browser WebRTC and
attaching to an already-created provider call are separate transport modes and are not advertised
by Astro's public contract.

## Lifecycle

1. The desktop subscribes to thread events and submits `RealtimeConversationStart`.
2. The server resumes or creates the thread runtime, installs the selected in-memory chat target,
   and submits the protocol operation.
3. The session builds bounded startup context, opens `/v1/realtime?model=...`, waits for
   `session.created`, sends the GA `session.update`, and publishes a started event containing the
   provider session id.
4. The desktop captures mono microphone samples, resamples them to PCM16 at 24 kHz, and sends
   100 ms frames through a bounded queue. Server VAD creates and interrupts responses.
5. Raw provider events travel through the unified thread stream. The desktop renders transcript
   deltas and schedules PCM output without gaps. Local playback is cancelled when user speech
   starts, enabling barge-in.
6. Explicit stop, session navigation, provider errors, component teardown, and runtime shutdown
   all release microphone, audio, queue, WebSocket, and task resources.

## Public operations

- start a realtime conversation
- append audio
- append role-bearing text
- request speech output
- close the conversation
- list supported voices

Only lifecycle boundaries are durable rollout events. High-volume audio and delta events remain
transient so the append-only rollout never stores raw microphone or synthesized audio payloads.

## Limits and validation

- one active realtime connection per thread runtime
- OpenAI backend with a configured API key in the desktop surface
- mono PCM16 input at 24 kHz
- audio frames capped at 1 MiB
- 256-entry bounded Rust input/output queues
- 64-frame bounded browser send backlog
- 15-second connect/handshake timeout and 2-second close timeout
- startup instructions capped at 24,000 characters; up to 32 recent user/assistant messages,
  each capped at 8,000 characters

## OpenAI GA mapping

- endpoint: `wss://api.openai.com/v1/realtime?model=gpt-realtime`
- authentication: `Authorization: Bearer ...`; no preview beta header
- input: `input_audio_buffer.append`
- configuration: `session.update` with nested `audio.input` and `audio.output`
- output audio: `response.output_audio.delta`
- output transcript: `response.output_audio_transcript.delta`
- output text: `response.output_text.delta`

References:

- <https://developers.openai.com/api/docs/guides/realtime/>
- <https://developers.openai.com/api/docs/guides/realtime-webrtc/>
