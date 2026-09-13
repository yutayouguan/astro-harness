import parseAPNG from "apng-js";
import { motionSequence, type PetMotionClip } from "./petMotionClip.ts";

export type PetApng = {
  frames: HTMLCanvasElement[];
  durations: number[];
  frameCount: number;
  width: number;
  height: number;
};

/** Check bounds before handing untrusted chunks to the third-party parser. */
export function inspectPetApng(bytes: ArrayBuffer) {
  const view = new DataView(bytes);
  if (
    bytes.byteLength < 57 ||
    bytes.byteLength > 16 * 1024 * 1024 ||
    view.getUint32(0) !== 0x89504e47 ||
    view.getUint32(4) !== 0x0d0a1a0a
  )
    throw new Error("Invalid APNG file");
  let frames = 0,
    controls = 0,
    width = 0,
    height = 0,
    end = false;
  for (let offset = 8; offset < bytes.byteLength; ) {
    if (offset + 12 > bytes.byteLength) throw new Error("Truncated PNG chunk");
    const size = view.getUint32(offset);
    if (size > bytes.byteLength - offset - 12)
      throw new Error("Truncated PNG data");
    const type = view.getUint32(offset + 4);
    if (offset === 8 && (type !== 0x49484452 || size !== 13))
      throw new Error("Missing PNG header");
    if (type === 0x49484452) {
      if (offset !== 8 || size !== 13) throw new Error("Invalid PNG header");
      width = view.getUint32(offset + 8);
      height = view.getUint32(offset + 12);
      if (![192, 256].includes(width) || height !== 208)
        throw new Error("Pet APNG must use a 192/256x208 canvas");
    } else if (type === 0x6163544c) {
      if (size !== 8 || frames || controls)
        throw new Error("Invalid animation control");
      frames = view.getUint32(offset + 8);
      if (frames < 2 || frames > 128)
        throw new Error("APNG frame limit exceeded");
    } else if (type === 0x6663544c) {
      if (!frames || size !== 26 || ++controls > frames)
        throw new Error("Invalid frame control");
      const w = view.getUint32(offset + 12),
        h = view.getUint32(offset + 16);
      const x = view.getUint32(offset + 20),
        y = view.getUint32(offset + 24);
      if (
        !w ||
        !h ||
        x + w > width ||
        y + h > height ||
        view.getUint8(offset + 32) > 2 ||
        view.getUint8(offset + 33) > 1
      )
        throw new Error("APNG frame outside canvas");
    } else if (type === 0x49454e44) {
      if (size !== 0 || offset + 12 !== bytes.byteLength)
        throw new Error("Invalid PNG end");
      end = true;
    }
    offset += size + 12;
  }
  if (!end || !frames || controls !== frames)
    throw new Error("Incomplete APNG animation");
  return { width, height, frames };
}

export function apngFrameAt(
  durations: readonly number[],
  elapsed: number,
  repeat: boolean,
  clip?: PetMotionClip,
  holdBody = false,
) {
  if (holdBody && clip) {
    const entry = durations.slice(0, clip.loopStart).reduce((sum, time) => sum + time, 0);
    const body = durations.slice(clip.loopStart, clip.loopEnd).reduce((sum, time) => sum + time, 0);
    let cursor = Math.max(0, Number.isFinite(elapsed) ? elapsed : 0);
    const indices = cursor < entry
      ? Array.from({ length: clip.loopStart }, (_, index) => index)
      : Array.from({ length: clip.loopEnd - clip.loopStart }, (_, index) => index + clip.loopStart);
    if (cursor >= entry) cursor = (cursor - entry) % body;
    for (const index of indices) {
      if (cursor < durations[index]) return { index, done: false, waitMs: durations[index] - cursor };
      cursor -= durations[index];
    }
  }
  const sequence = clip
    ? motionSequence(clip)
    : durations.map((_, index) => index);
  const total = sequence.reduce((sum, index) => sum + durations[index], 0);
  let cursor = Math.max(0, Number.isFinite(elapsed) ? elapsed : 0);
  if (repeat) cursor %= total;
  for (const index of sequence) {
    if (cursor < durations[index]) return { index, done: false, waitMs: durations[index] - cursor };
    cursor -= durations[index];
  }
  return { index: sequence[sequence.length - 1], done: true, waitMs: Infinity };
}

export async function decodePetApng(
  bytes: ArrayBuffer,
  poster = false,
): Promise<PetApng> {
  inspectPetApng(bytes);
  const animation = parseAPNG(bytes);
  if (animation instanceof Error) throw animation;
  const selected = poster ? animation.frames.slice(0, 1) : animation.frames;
  await Promise.all(selected.map((frame) => frame.createImage()));
  const canvas = document.createElement("canvas");
  canvas.width = animation.width;
  canvas.height = animation.height;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) throw new Error("Canvas unavailable");
  const frames: HTMLCanvasElement[] = [];
  for (const frame of selected) {
    const previous =
      frame.disposeOp === 2
        ? context.getImageData(0, 0, canvas.width, canvas.height)
        : null;
    if (frame.blendOp === 0)
      context.clearRect(frame.left, frame.top, frame.width, frame.height);
    context.drawImage(frame.imageElement!, frame.left, frame.top);
    const snapshot = document.createElement("canvas");
    snapshot.width = canvas.width;
    snapshot.height = canvas.height;
    const target = snapshot.getContext("2d");
    if (!target) throw new Error("Frame canvas unavailable");
    target.drawImage(canvas, 0, 0);
    frames.push(snapshot);
    if (frame.disposeOp === 1)
      context.clearRect(frame.left, frame.top, frame.width, frame.height);
    else if (previous) context.putImageData(previous, 0, 0);
  }
  return {
    frames,
    durations: animation.frames.map((frame) => frame.delay),
    frameCount: animation.frames.length,
    width: animation.width,
    height: animation.height,
  };
}
