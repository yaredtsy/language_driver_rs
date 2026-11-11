const api = {
  load() {},
  async save(x: number) {},
  get size() {
    return 1;
  },
  set size(v: number) {},
  arrowProp: () => {},
  functionProp: function () {},
  nested: {
    deep() {},
    deeper: {
      deepest() {},
    },
  },
  "quoted-key"() {},
  [computedKey]() {},
  shorthand,
};

export default {
  fromDefault() {},
};

registerPlugin({
  anonymousMethod() {},
});

function build() {
  return {
    made() {},
  };
}

let mutable = {
  stillADef() {},
};

class Holder {
  static config = {
    read() {},
  };
}
