export type AtlasImage = {
  src: string;
  naturalWidth: number;
  naturalHeight: number;
  onload: ((event: Event) => unknown) | null;
  onerror: ((event: Event | string) => unknown) | null;
};

export function loadPetAtlas<T extends AtlasImage>(options: {
  src: string;
  createImage: () => T;
  loaded: (image: T) => void;
  failed: () => void;
  kind?: "v2" | "grooming";
}) {
  const image = options.createImage();
  let disposed = false;
  image.onload = () => {
    if (disposed) return;
    const width = options.kind === "grooming" ? 1152 : 1536;
    const height = options.kind === "grooming" ? 208 : 2288;
    if (image.naturalWidth !== width || image.naturalHeight !== height)
      options.failed();
    else options.loaded(image);
  };
  image.onerror = () => {
    if (!disposed) options.failed();
  };
  image.src = options.src;
  return () => {
    disposed = true;
    image.onload = null;
    image.onerror = null;
  };
}
