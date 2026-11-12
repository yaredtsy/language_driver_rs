export const api = {
  load: () => helper(),
  tasks: {
    update: async (id: string) => {
      helper();
    },
  },
};

export const wrapped = { a: () => helper() } as const;

export const documented = {
  /** ID: ARROW-0001 */
  marked: () => {},
};

// not defs (D21): a `let`, a default export, a class property's literal
export let mutable = { a: () => {} };

export default { fromDefault: () => {} };

export class Holder {
  static config = { read: () => {} };
}

export const left = { go: () => {} };
export const right = { go: () => {} };

function helper() {}
