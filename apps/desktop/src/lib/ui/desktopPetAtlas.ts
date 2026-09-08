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
}) {
  const image = options.createImage();
  let disposed = false;
  image.onload = () => {
    if (disposed) return;
    if (image.naturalWidth !== 1536 || image.naturalHeight !== 2288)
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
