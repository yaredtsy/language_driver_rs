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
