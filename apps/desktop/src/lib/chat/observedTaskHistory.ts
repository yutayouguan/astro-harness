/** Read-only history observer: never starts, stops or resumes a backend turn. */
export function createObservedTaskHistory<T>(options: {
  load: () => Promise<T>;
  isCurrent: () => boolean;
  commit: (history: T) => void;
}) {
  let disposed = false;
  let busy = false;
  return {
    async refresh() {
      if (disposed || busy || !options.isCurrent()) return;
      busy = true;
      try {
        const history = await options.load();
        if (!disposed && options.isCurrent()) options.commit(history);
      } catch {
        // A disconnected history read must not clear the current view or drafts.
        // The next reconciliation tick retries.
      } finally {
        busy = false;
      }
    },
    dispose() {
      disposed = true;
    },
  };
}
