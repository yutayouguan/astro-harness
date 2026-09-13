import {
  canUndoPetScale,
  type PetScaleUndoSnapshot,
} from "./ambienceSession.ts";

export type LivePetScaleRequest = {
  petId: string;
  scale: number;
  gestureId: string;
};
export type PetScaleGesture = { gestureId: string; undo: PetScaleUndoSnapshot };

export function petScaleGestureUndo(
  previous: PetScaleGesture | null,
  request: LivePetScaleRequest,
  current: { activePetId?: string | null; scale: number },
  appliedScale: number,
): PetScaleGesture {
  return {
    gestureId: request.gestureId,
    undo: {
      kind: "pet-scale",
      petId: request.petId,
      before:
        previous?.gestureId === request.gestureId &&
        canUndoPetScale(previous.undo, current)
          ? previous.undo.before
          : current.scale,
      expected: appliedScale,
    },
  };
}

/** Start on input, not release. Never overlap native resizes or replay stale intermediate sizes. */
export function createLivePetScaleQueue(options: {
  apply: (request: LivePetScaleRequest) => Promise<boolean>;
  busy: (value: boolean) => void;
  settled: (ok: boolean) => void;
}) {
  let pending = false;
  let latest: LivePetScaleRequest | null = null;
  let completion = Promise.resolve();
  async function drain() {
    pending = true;
    options.busy(true);
    let ok = true;
    try {
      while (latest) {
        const request = latest;
        latest = null;
        if (!(await options.apply(request))) {
          ok = false;
          latest = null;
          break;
        }
      }
    } catch {
      ok = false;
      latest = null;
    } finally {
      pending = false;
      options.busy(false);
      options.settled(ok);
    }
  }
  return {
    isPending: () => pending,
    enqueue: (request: LivePetScaleRequest) => {
      latest = request;
      if (!pending) completion = drain();
      return completion;
    },
  };
}
