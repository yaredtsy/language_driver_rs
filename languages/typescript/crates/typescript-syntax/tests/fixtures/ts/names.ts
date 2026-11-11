class Names {
  plain() {}
  "my-key"() {}
  'single'() {}
  42() {}
  [Symbol.iterator]() {}
  ["literal-computed"]() {}
  #secret() {}
  get #hiddenGetter() {
    return 1;
  }
  static shared() {}
  shared() {}
  delete() {}
  class() {}
  café() {}
  \u0061scii() {}
  get both() {
    return 1;
  }
  set both(v: number) {}
}

const objectWithNames = {
  "string key"() {},
  123() {},
  [computed + 1]() {},
};

const quoted = {
  "load"() {},
  "class"() {},
  "a.b"() {},
  "1st"() {},
  ""() {},
  "caf\u00e9"() {},
  "nested": {
    deep() {},
  },
  "not nested": {
    deep() {},
  },
};

declare module "lodash" {
  function chunk(): void;
}

declare module "@scope/pkg" {
  function scoped(): void;
}
