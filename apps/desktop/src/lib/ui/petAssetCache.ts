type Entry<T> = {
  controller: AbortController;
  promise: Promise<T>;
  users: number;
  bytes: number;
  touched: number;
  settled: boolean;
};

/** Leased LRU: live canvases and in-flight decodes are charged, never evicted. */
export function createPetAssetCache<T>(
  capacity: number,
  load: (
    key: string,
    signal: AbortSignal,
    reserve: (bytes: number) => void,
  ) => Promise<T>,
) {
  const entries = new Map<string, Entry<T>>();
  let bytes = 0,
    tick = 0;
  const uncharge = (entry: Entry<T>) => {
    bytes -= entry.bytes;
    entry.bytes = 0;
  };
  return {
    stats: () => ({ bytes, entries: entries.size }),
    acquire(key: string) {
      let entry = entries.get(key);
      if (!entry) {
        const controller = new AbortController();
        const created: Entry<T> = {
          controller,
          users: 0,
          bytes: 0,
          touched: ++tick,
          settled: false,
          promise: Promise.resolve(null as T),
        };
        entries.set(key, created);
        created.promise = Promise.resolve()
          .then(() =>
            load(key, controller.signal, (size) => {
              if (controller.signal.aborted)
                throw new Error("Pet load cancelled");
              if (
                !Number.isSafeInteger(size) ||
                size < 1 ||
                size > capacity ||
                created.bytes
              )
                throw new Error("Invalid pet decode allocation");
              const idle = [...entries.entries()]
                .filter(
                  ([, value]) =>
                    value !== created && !value.users && value.settled,
                )
                .sort((a, b) => a[1].touched - b[1].touched);
              for (const [oldKey, old] of idle) {
                if (bytes + size <= capacity) break;
                entries.delete(oldKey);
                uncharge(old);
              }
              if (bytes + size > capacity)
                throw new Error("Pet decode memory budget exceeded");
              created.bytes = size;
              bytes += size;
            }),
          )
          .then((value) => {
            if (controller.signal.aborted)
              throw new Error("Pet load cancelled");
            created.settled = true;
            return value;
          })
          .catch((error) => {
            created.settled = true;
            uncharge(created);
            if (entries.get(key) === created) entries.delete(key);
            throw error;
          });
        // The lease owner handles the error; a disposed owner must not cause an unhandled rejection.
        void created.promise.catch(() => {});
        entry = created;
      }
      const leased = entry;
      leased.users++;
      leased.touched = ++tick;
      let released = false;
      return {
        ready: leased.promise,
        release() {
          if (released) return;
          released = true;
          leased.users--;
          leased.touched = ++tick;
          if (!leased.users && !leased.settled) {
            leased.controller.abort();
            if (entries.get(key) === leased) entries.delete(key);
            // Keep the allocation charged until an uncancellable decode actually settles.
          }
        },
      };
    },
  };
}
