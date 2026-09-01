import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";

const OUTPUT_SAMPLE_RATE = 24_000;
const SEND_FRAME_SAMPLES = 2_400;
const MAX_QUEUED_AUDIO_FRAMES = 64;

export type RealtimeConversationStatus =
  "idle" | "connecting" | "active" | "stopping" | "error";

type RealtimeConversationEvent = {
  sessionId: string;
  kind: "started" | "sdp" | "event" | "closed" | "voices";
  payload: Record<string, unknown>;
};

type Options = {
  sessionId: string | null;
  providerId: string | null;
  backendId: string | null;
  model?: string;
  voice?: string;
  transport?: "webrtc" | "websocket" | "existing_call";
  callId?: string;
  version?: "v2" | "v3";
  clientManagedHandoffs?: boolean;
  handoffMode?: "thinking" | "commentary" | "bem_tags";
  handoffChannelPrefixes?: Record<string, string[]>;
  flushTranscriptTailOnSessionEnd?: boolean;
};

type CaptureHandle = {
  context: AudioContext;
  stream: MediaStream;
  source: MediaStreamAudioSourceNode;
  processor: AudioNode;
  sink: GainNode;
  objectUrl?: string;
};

type WebRtcHandle = {
  peer: RTCPeerConnection;
  stream: MediaStream;
  audio: HTMLAudioElement;
  events: RTCDataChannel;
};

async function waitForIceGathering(peer: RTCPeerConnection): Promise<void> {
  if (peer.iceGatheringState === "complete") return;
  await new Promise<void>((resolve) => {
    const timeout = window.setTimeout(() => {
      peer.removeEventListener("icegatheringstatechange", onChange);
      resolve();
    }, 5_000);
    const onChange = () => {
      if (peer.iceGatheringState !== "complete") return;
      window.clearTimeout(timeout);
      peer.removeEventListener("icegatheringstatechange", onChange);
      resolve();
    };
    peer.addEventListener("icegatheringstatechange", onChange);
  });
}

async function createWebRtc(): Promise<WebRtcHandle> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: {
      channelCount: 1,
      echoCancellation: true,
      noiseSuppression: true,
      autoGainControl: true,
    },
  });
  const peer = new RTCPeerConnection();
  stream.getTracks().forEach((track) => peer.addTrack(track, stream));
  const events = peer.createDataChannel("oai-events");
  const audio = new Audio();
  audio.autoplay = true;
  peer.ontrack = ({ streams }) => {
    audio.srcObject = streams[0] ?? new MediaStream();
    void audio.play().catch(() => undefined);
  };
  const offer = await peer.createOffer();
  await peer.setLocalDescription(offer);
  await waitForIceGathering(peer);
  return { peer, stream, audio, events };
}

function stopWebRtc(handle: WebRtcHandle | null): void {
  if (!handle) return;
  handle.events.close();
  handle.peer.close();
  handle.stream.getTracks().forEach((track) => track.stop());
  handle.audio.pause();
  handle.audio.srcObject = null;
}

export function resampleFloat32ToPcm16(
  input: Float32Array,
  inputRate: number,
  outputRate = OUTPUT_SAMPLE_RATE,
): Uint8Array {
  if (input.length === 0 || inputRate <= 0 || outputRate <= 0) {
    return new Uint8Array();
  }
  const outputLength = Math.max(
    1,
    Math.round((input.length * outputRate) / inputRate),
  );
  const bytes = new Uint8Array(outputLength * 2);
  const view = new DataView(bytes.buffer);
  const ratio = inputRate / outputRate;
  for (let index = 0; index < outputLength; index += 1) {
    const position = Math.min(input.length - 1, index * ratio);
    const left = Math.floor(position);
    const right = Math.min(input.length - 1, left + 1);
    const fraction = position - left;
    const sample = Math.max(
      -1,
      Math.min(1, input[left] + (input[right] - input[left]) * fraction),
    );
    view.setInt16(
      index * 2,
      sample < 0 ? sample * 0x8000 : sample * 0x7fff,
      true,
    );
  }
  return bytes;
}

function decodeBase64Pcm16(value: string): Float32Array {
  const raw = atob(value);
  const bytes = new Uint8Array(raw.length);
  for (let index = 0; index < raw.length; index += 1) {
    bytes[index] = raw.charCodeAt(index);
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const samples = new Float32Array(Math.floor(bytes.byteLength / 2));
  for (let index = 0; index < samples.length; index += 1) {
    const sample = view.getInt16(index * 2, true);
    samples[index] = sample / (sample < 0 ? 0x8000 : 0x7fff);
  }
  return samples;
}

async function createCapture(
  onSamples: (samples: Float32Array, sampleRate: number) => void,
): Promise<CaptureHandle> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: {
      channelCount: 1,
      echoCancellation: true,
      noiseSuppression: true,
      autoGainControl: true,
    },
  });
  const context = new AudioContext();
  await context.resume();
  const source = context.createMediaStreamSource(stream);
  const sink = context.createGain();
  sink.gain.value = 0;
  sink.connect(context.destination);

  if (context.audioWorklet) {
    const sourceCode = `
      class AstroRealtimePcmProcessor extends AudioWorkletProcessor {
        process(inputs) {
          const input = inputs[0] && inputs[0][0];
          if (input && input.length) this.port.postMessage(input.slice());
          return true;
        }
      }
      registerProcessor("astro-realtime-pcm", AstroRealtimePcmProcessor);
    `;
    const objectUrl = URL.createObjectURL(
      new Blob([sourceCode], { type: "text/javascript" }),
    );
    try {
      await context.audioWorklet.addModule(objectUrl);
      const processor = new AudioWorkletNode(context, "astro-realtime-pcm");
      processor.port.onmessage = ({ data }: MessageEvent<Float32Array>) =>
        onSamples(data, context.sampleRate);
      source.connect(processor);
      processor.connect(sink);
      return { context, stream, source, processor, sink, objectUrl };
    } catch {
      URL.revokeObjectURL(objectUrl);
    }
  }

  const processor = context.createScriptProcessor(4096, 1, 1);
  processor.onaudioprocess = (event) =>
    onSamples(event.inputBuffer.getChannelData(0).slice(), context.sampleRate);
  source.connect(processor);
  processor.connect(sink);
  return { context, stream, source, processor, sink };
}

async function stopCapture(handle: CaptureHandle | null): Promise<void> {
  if (!handle) return;
  handle.source.disconnect();
  handle.processor.disconnect();
  handle.sink.disconnect();
  handle.stream.getTracks().forEach((track) => track.stop());
  await handle.context.close().catch(() => undefined);
  if (handle.objectUrl) URL.revokeObjectURL(handle.objectUrl);
}

export function useRealtimeConversation({
  sessionId,
  providerId,
  backendId,
  model = "gpt-realtime",
  voice = "marin",
  transport = "webrtc",
  callId,
  version = "v2",
  clientManagedHandoffs = false,
  handoffMode = "thinking",
  handoffChannelPrefixes,
  flushTranscriptTailOnSessionEnd = true,
}: Options) {
  const [status, setStatus] = useState<RealtimeConversationStatus>("idle");
  const [transcript, setTranscript] = useState("");
  const [error, setError] = useState<string | null>(null);
  const captureRef = useRef<CaptureHandle | null>(null);
  const webRtcRef = useRef<WebRtcHandle | null>(null);
  const sdpResolverRef = useRef<((sdp: string) => void) | null>(null);
  const playbackContextRef = useRef<AudioContext | null>(null);
  const playbackCursorRef = useRef(0);
  const pendingAudioRef = useRef(new Uint8Array());
  const sendChainRef = useRef<Promise<unknown>>(Promise.resolve());
  const queuedAudioFramesRef = useRef(0);
  const terminalErrorRef = useRef(false);
  const attemptRef = useRef(0);
  const desiredActiveRef = useRef(false);
  const sessionRef = useRef(sessionId);
  const activeSessionRef = useRef<string | null>(null);
  sessionRef.current = sessionId;

  const stopPlayback = useCallback(async () => {
    const playback = playbackContextRef.current;
    playbackContextRef.current = null;
    playbackCursorRef.current = 0;
    await playback?.close().catch(() => undefined);
  }, []);

  const releaseMedia = useCallback(async () => {
    const capture = captureRef.current;
    captureRef.current = null;
    pendingAudioRef.current = new Uint8Array();
    const webRtc = webRtcRef.current;
    webRtcRef.current = null;
    sdpResolverRef.current = null;
    stopWebRtc(webRtc);
    await stopCapture(capture);
    await stopPlayback();
  }, [stopPlayback]);

  const playPcm = useCallback(async (encoded: string) => {
    const samples = decodeBase64Pcm16(encoded);
    if (samples.length === 0) return;
    const context =
      playbackContextRef.current ??
      new AudioContext({ sampleRate: OUTPUT_SAMPLE_RATE });
    playbackContextRef.current = context;
    await context.resume();
    const buffer = context.createBuffer(1, samples.length, OUTPUT_SAMPLE_RATE);
    buffer.getChannelData(0).set(samples);
    const source = context.createBufferSource();
    source.buffer = buffer;
    source.connect(context.destination);
    const startsAt = Math.max(context.currentTime, playbackCursorRef.current);
    source.start(startsAt);
    playbackCursorRef.current = startsAt + buffer.duration;
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen<RealtimeConversationEvent>(
      "realtime_conversation_event",
      ({ payload }) => {
        if (payload.sessionId !== sessionRef.current) return;
        if (payload.kind === "started") {
          if (!desiredActiveRef.current) {
            void invoke("close_realtime_conversation", {
              sessionId: payload.sessionId,
            });
            return;
          }
          activeSessionRef.current = payload.sessionId;
          if (!webRtcRef.current) setStatus("active");
          setError(null);
          return;
        }
        if (payload.kind === "sdp") {
          const sdp =
            typeof payload.payload.sdp === "string" ? payload.payload.sdp : "";
          if (sdp) sdpResolverRef.current?.(sdp);
          return;
        }
        if (payload.kind === "closed") {
          attemptRef.current += 1;
          desiredActiveRef.current = false;
          activeSessionRef.current = null;
          void releaseMedia();
          const reason =
            typeof payload.payload.reason === "string"
              ? payload.payload.reason
              : "";
          if (terminalErrorRef.current) {
            terminalErrorRef.current = false;
            setStatus("error");
          } else if (
            reason &&
            reason !== "requested" &&
            reason !== "cancelled"
          ) {
            setError(reason);
            setStatus("error");
          } else {
            setStatus("idle");
          }
          return;
        }
        if (payload.kind !== "event") return;
        const type =
          typeof payload.payload.type === "string" ? payload.payload.type : "";
        const data =
          payload.payload.data && typeof payload.payload.data === "object"
            ? (payload.payload.data as Record<string, unknown>)
            : {};
        const delta =
          typeof data.delta === "string"
            ? data.delta
            : "";
        if (type === "input_audio_speech_started") {
          void stopPlayback();
        }
        if (type === "audio_out" && !webRtcRef.current) {
          const bytes = Array.isArray(data.data)
            ? new Uint8Array(data.data as number[])
            : null;
          if (bytes) {
            let binary = "";
            bytes.forEach((byte) => {
              binary += String.fromCharCode(byte);
            });
            void playPcm(btoa(binary)).catch(() => undefined);
          }
        }
        if (
          delta &&
          (type === "input_transcript_delta" ||
            type === "output_transcript_delta")
        ) {
          setTranscript((current) => current + delta);
        }
        if (type === "error") {
          const message = typeof payload.payload.data === "string"
            ? payload.payload.data
            : "Realtime 会话发生错误";
          setError(message);
          setStatus("error");
          attemptRef.current += 1;
          desiredActiveRef.current = false;
          terminalErrorRef.current = true;
          activeSessionRef.current = null;
          void releaseMedia();
          void invoke("close_realtime_conversation", {
            sessionId: payload.sessionId,
          });
        }
      },
    ).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      attemptRef.current += 1;
      desiredActiveRef.current = false;
      unlisten?.();
      void releaseMedia();
      const activeSession = activeSessionRef.current;
      activeSessionRef.current = null;
      if (activeSession) {
        void invoke("close_realtime_conversation", {
          sessionId: activeSession,
        });
      }
    };
  }, [playPcm, releaseMedia, stopPlayback]);

  useEffect(() => {
    const activeSession = activeSessionRef.current;
    if (!activeSession || activeSession === sessionId) return;
    attemptRef.current += 1;
    desiredActiveRef.current = false;
    activeSessionRef.current = null;
    void releaseMedia();
    void invoke("close_realtime_conversation", {
      sessionId: activeSession,
    });
    setStatus("idle");
    setTranscript("");
  }, [releaseMedia, sessionId]);

  const enqueueAudio = useCallback(
    (samples: Float32Array, sampleRate: number) => {
      const sid = sessionRef.current;
      if (!sid) return;
      const next = resampleFloat32ToPcm16(samples, sampleRate);
      const pending = new Uint8Array(
        pendingAudioRef.current.length + next.length,
      );
      pending.set(pendingAudioRef.current);
      pending.set(next, pendingAudioRef.current.length);
      pendingAudioRef.current = pending;
      const frameBytes = SEND_FRAME_SAMPLES * 2;
      while (pendingAudioRef.current.length >= frameBytes) {
        const frame = pendingAudioRef.current.slice(0, frameBytes);
        pendingAudioRef.current = pendingAudioRef.current.slice(frameBytes);
        if (queuedAudioFramesRef.current >= MAX_QUEUED_AUDIO_FRAMES) continue;
        queuedAudioFramesRef.current += 1;
        const attempt = attemptRef.current;
        sendChainRef.current = sendChainRef.current
          .catch(() => undefined)
          .then(() => {
            if (attempt !== attemptRef.current || !desiredActiveRef.current) {
              return undefined;
            }
            return invoke("send_realtime_audio", {
              sessionId: sid,
              data: Array.from(frame),
            });
          })
          .finally(() => {
            queuedAudioFramesRef.current -= 1;
          });
      }
    },
    [],
  );

  const stop = useCallback(async () => {
    attemptRef.current += 1;
    desiredActiveRef.current = false;
    const sid = activeSessionRef.current ?? sessionRef.current;
    setStatus("stopping");
    await releaseMedia();
    await sendChainRef.current.catch(() => undefined);
    if (sid) {
      await invoke("close_realtime_conversation", { sessionId: sid }).catch(
        () => undefined,
      );
    }
    activeSessionRef.current = null;
    setStatus("idle");
  }, [releaseMedia]);

  const start = useCallback(async () => {
    if (!sessionId || !providerId || !backendId) {
      setError("当前模型没有可用的 Realtime 凭据");
      setStatus("error");
      return;
    }
    setError(null);
    terminalErrorRef.current = false;
    desiredActiveRef.current = true;
    setTranscript("");
    setStatus("connecting");
    const attempt = ++attemptRef.current;
    activeSessionRef.current = sessionId;
    try {
      const webRtc = transport === "webrtc" ? await createWebRtc() : null;
      if (attempt !== attemptRef.current) {
        stopWebRtc(webRtc);
        return;
      }
      webRtcRef.current = webRtc;
      const offerSdp = webRtc?.peer.localDescription?.sdp;
      if (webRtc && !offerSdp) throw new Error("WebRTC 未生成 SDP offer");
      const answer = webRtc
        ? new Promise<string>((resolve, reject) => {
            const timeout = window.setTimeout(() => {
              sdpResolverRef.current = null;
              reject(new Error("等待 Realtime SDP answer 超时"));
            }, 20_000);
            sdpResolverRef.current = (sdp) => {
              window.clearTimeout(timeout);
              sdpResolverRef.current = null;
              resolve(sdp);
            };
          })
        : null;
      await invoke("start_realtime_conversation", {
        request: {
          sessionId,
          provider: backendId,
          providerId,
          model,
          voice,
          outputModality: "audio",
          turnDetection: "server_vad",
          noiseReduction: "near_field",
          includeStartupContext: transport !== "existing_call",
          transport,
          sdp: offerSdp ?? null,
          callId: callId ?? null,
          version,
          clientManagedHandoffs,
          handoffMode,
          handoffChannelPrefixes: handoffChannelPrefixes ?? null,
          flushTranscriptTailOnSessionEnd,
        },
      });
      if (attempt !== attemptRef.current) {
        await invoke("close_realtime_conversation", { sessionId }).catch(
          () => undefined,
        );
        return;
      }
      if (webRtc && answer) {
        const answerSdp = await answer;
        if (attempt !== attemptRef.current) return;
        await webRtc.peer.setRemoteDescription({ type: "answer", sdp: answerSdp });
      } else if (transport === "websocket") {
        const capture = await createCapture(enqueueAudio);
        if (attempt !== attemptRef.current) {
          await stopCapture(capture);
          return;
        }
        captureRef.current = capture;
      }
      setStatus("active");
    } catch (cause) {
      desiredActiveRef.current = false;
      activeSessionRef.current = null;
      await releaseMedia();
      await invoke("close_realtime_conversation", { sessionId }).catch(
        () => undefined,
      );
      if (attempt !== attemptRef.current) return;
      setError(String(cause));
      setStatus("error");
    }
  }, [
    backendId,
    callId,
    clientManagedHandoffs,
    enqueueAudio,
    flushTranscriptTailOnSessionEnd,
    handoffChannelPrefixes,
    handoffMode,
    model,
    providerId,
    releaseMedia,
    sessionId,
    transport,
    version,
    voice,
  ]);

  const toggle = useCallback(() => {
    if (status === "idle" || status === "error") void start();
    else void stop();
  }, [start, status, stop]);

  return {
    status,
    transcript,
    error,
    active: status === "active" || status === "connecting",
    start,
    stop,
    toggle,
  };
}
