// Defs written where they are reached without a direct call (H12): a class expression under `new`, and a function expression under `.call` / `.apply`.
function g() {}

// (a) a named class expression is a def, and `new` of it is a construction, like `new C()`
export function newNamedClass() {
  new (class N {
    constructor() {
      g();
    }
  })();
}
// the instance initializer runs at the construction, so it is counted there and not drawn, as for any class
export function newNamedClassWithField() {
  new (class M {
    x = g();
  })();
}
// under parentheses and an assertion
export function newAssertedClass() {
  new (<any>class A {
    constructor() {
      g();
    }
  })();
}
// an anonymous class is no def: unchanged
export function newAnonymousClass() {
  new (class {})();
}

// (b) a named function expression reached through `.call` or `.apply`: its calls are in no tree here, so it is counted
export function callWrapped() {
  (function w() {
    g();
  }).call(null);
}
export function applyWrapped() {
  (function w2() {
    g();
  }).apply(null, []);
}
// an anonymous one is no def: its calls are this body's, and nothing is lost
export function callAnonymous() {
  (function () {
    g();
  }).call(null);
}
// a named function that is called by name is counted already
export function callNamed() {
  function named() {
    g();
  }
  named.call(null);
}
