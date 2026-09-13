import naitang from "../../assets/pets/naitang/idle-rig/rig.json";
import pudding from "../../assets/pets/pudding/idle-rig/rig.json";
import { acquirePetApng } from "./petApngCache";
import { resolveMediaSrc } from "../media/resolveMediaSrc";
import { fingerprintPixels } from "./petIdleRigMotion";

export type IdleRigConfig = typeof naitang;
export type IdleRig = {
  config: IdleRigConfig;
  canvasWidth?: number;
  images: Record<"neutral" | "body" | "tail" | "heads", HTMLImageElement>;
};
const configs = [naitang, pudding];
const files = import.meta.glob("../../assets/pets/*/idle-rig/*.png", {
  eager: true,
  query: "?url",
  import: "default",
});
const rigs = new Map<string, Promise<IdleRig>>();
const probes = new Map<string, Promise<IdleRig | null>>();
function image(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("Idle rig image unavailable"));
    img.src = url;
  });
}
function loadRig(config: IdleRigConfig) {
  let promise = rigs.get(config.pet);
  if (!promise) {
    promise = Promise.all(
      (["neutral", "body", "tail", "heads"] as const).map(async (name) => {
        const url = files[
          `../../assets/pets/${config.pet}/idle-rig/${name}.png`
        ] as string;
        const img = await image(url);
        if (
          img.naturalWidth !== (name === "heads" ? 1728 : 192) ||
          img.naturalHeight !== 208
        )
          throw new Error("Invalid idle rig canvas");
        return [name, img] as const;
      }),
    ).then((entries) => ({
      config,
      images: Object.fromEntries(entries) as IdleRig["images"],
    }));
    rigs.set(config.pet, promise);
    void promise.catch(() => rigs.delete(config.pet));
  }
  return promise;
}
/** Never select a rig by pet name: copied/renamed/custom artwork must match. */
export function probeIdleRig(src: string): Promise<IdleRig | null> {
  let promise = probes.get(src);
  if (promise) return promise;
  promise = (async () => {
    const url = resolveMediaSrc(src) || src;
    const apng = /\.apng(?:[?#]|$)/i.test(src);
    // Legacy arbitrary imported atlases do not need another 14MiB decode.
    if (
      !apng &&
      !/builtin-(naitang|pudding)-/.test(src) &&
      !/spritesheet/.test(src)
    )
      return null;
    const lease = apng ? acquirePetApng(url, true) : null;
    try {
      const decoded = lease ? await lease.ready : null;
      const source = decoded ? decoded.frames[0] : await image(url);
      const width = decoded?.width ?? (source as HTMLImageElement).naturalWidth;
      if (
        !apng &&
        (width !== 1536 || (source as HTMLImageElement).naturalHeight !== 2288)
      )
        return null;
      const canvas = document.createElement("canvas");
      canvas.width = 192;
      canvas.height = 208;
      const ctx = canvas.getContext("2d", { willReadFrequently: true });
      if (!ctx) return null;
      ctx.drawImage(
        source,
        apng && width === 256 ? 32 : 0,
        0,
        192,
        208,
        0,
        0,
        192,
        208,
      );
      const digest = await crypto.subtle.digest(
        "SHA-256",
        fingerprintPixels(ctx.getImageData(0, 0, 192, 208).data),
      );
      const hash = Array.from(new Uint8Array(digest), (byte) =>
        byte.toString(16).padStart(2, "0"),
      ).join("");
      const config = configs.find((item) => item.fingerprint === hash);
      return config
        ? { ...(await loadRig(config)), canvasWidth: apng ? width : 192 }
        : null;
    } finally {
      lease?.release();
    }
  })().catch(() => null);
  probes.set(src, promise);
  if (probes.size > 24) probes.delete(probes.keys().next().value!);
  return promise;
}
