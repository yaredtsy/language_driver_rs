const plainArrow = () => {};

export const exportedArrow = (a: number) => a + 1;

const fnExpression = function (x: number) {
  return x;
};

const ClassExpr = class {
  method() {}
};

let letArrow = () => {};

var varArrow = function () {};

const first = () => {},
  second = () => {};

const namedExpr = function inner() {};

const notFunction = 1;

const parenthesized = (() => {});

const { destructured } = { destructured: () => {} };

xs.map((x) => x * 2);

run(function step() {});

run(function () {
  function insideCallback() {}
});

class Component {
  handle = () => {};
  static make = function () {};
  #hidden = () => {};
  ["computed"] = () => {};
  plainField = 42;
  Inner = class {
    deep() {}
  };
}

function outer() {
  const local = () => {
    const nested = () => {};
  };
  callIt(function named() {});
}
