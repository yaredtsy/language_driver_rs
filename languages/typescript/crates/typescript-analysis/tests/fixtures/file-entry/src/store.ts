function create<S>(init: (set: (patch: Partial<S>) => void) => S): S {
  return init(() => {});
}

function log() {}

export const useStore = create<{ count: number; inc: () => void }>((set) => ({
  count: 0,
  inc: () => {
    set({ count: 1 });
    log();
  },
}));
