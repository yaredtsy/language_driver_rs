// D21: a property holding an arrow or a function expression is a def in a `const` object literal, and nowhere else.

declare const flag: boolean;
declare const computed: string;
declare const shorthand: () => void;
declare function register(o: unknown): void;
declare function create<T>(init: (set: unknown) => T): T;

// the plain case, beside methods and accessors, which were defs already
const api = {
  load: () => {},
  async save(x: number) {},
  fetchAll: async (id: string) => {},
  run: function () {},
  "quoted-key": () => {},
  "not valid": () => {},
  [computed]: () => {},
  1: () => {},
  shorthand,
  named: function step() {},
  count: 3,
  call: shorthand(),
};

// nested objects: the keys are the path
const tree = {
  tasks: {
    update: () => {},
    deeper: {
      remove: async () => {},
    },
  },
};

export const exported = {
  e: () => {},
};

function holder() {
  const local = {
    l: () => {},
  };
  return local;
}

// wrappers that keep the literal as the initializer: the arrows are defs, named with the variable
const asConst = { a: () => {} } as const;
const satisfying = { a: () => {} } satisfies Record<string, () => void>;
const parenthesised = ({ a: () => {} });
const asserted = <const>{ a: () => {} };
const both = ({ a: () => {} } as const) satisfies object;
// a method of a wrapped literal is named without the variable (frozen since TS1), next to arrows that are named with it
const wrappedMixed = {
  m() {},
  a: () => {},
  nested: { b: () => {} },
} as const;

// a JSDoc above the property is the def's block
const documented = {
  /** Loads. */
  load: () => {},
  /**
   * Saves.
   * @param x the thing
   */
  save: (x: number) => {},
};

// what is inside an arrow property keeps the names it had before the arrow was a def
const withInner = {
  load: () => {
    function inner() {}
    const helper = () => {};
    class Local {}
    return { made() {}, madeArrow: () => {} };
  },
};

// not defs: no stable name
let mutable = { a: () => {} };
var legacy = { a: () => {} };
export default { fromDefault: () => {} };
class Holder {
  static config = { read: () => {} };
}
register({ onSuccess: () => {} });
function build() {
  return { made: () => {} };
}
const list = [{ item: () => {} }];
const frozen = Object.freeze({ f: () => {} });
const store = create((set) => ({ inc: () => {} }));
const spread = { ...{ s: () => {} } };
const chosen = flag ? { c: () => {} } : { d: () => {} };
const inside = {
  items: [{ i: () => {} }],
  call: register({ x: () => {} }),
  wrapped: shorthand(),
};
const { destructured } = { destructured: () => {} };
