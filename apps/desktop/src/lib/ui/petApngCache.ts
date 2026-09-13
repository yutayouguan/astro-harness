import { createPetAssetCache } from "./petAssetCache";
import { decodePetApng, inspectPetApng } from "./petApng";

export const PET_APNG_CACHE_BYTES = 64 * 1024 * 1024;
const cache = createPetAssetCache(
  PET_APNG_CACHE_BYTES,
  async (key, signal, reserve) => {
    const [url, poster] = JSON.parse(key) as [string, boolean];
    const response = await fetch(url, { signal });
    if (!response.ok) throw new Error("APNG unavailable");
    if (Number(response.headers.get("content-length")) > 16 * 1024 * 1024)
      throw new Error("APNG file exceeds limit");
    const bytes = await response.arrayBuffer();
    const info = inspectPetApng(bytes);
    reserve(info.width * info.height * 4 * (poster ? 1 : info.frames));
    return decodePetApng(bytes, poster);
  },
);

export function acquirePetApng(url: string, poster: boolean) {
  return cache.acquire(JSON.stringify([url, poster]));
}
