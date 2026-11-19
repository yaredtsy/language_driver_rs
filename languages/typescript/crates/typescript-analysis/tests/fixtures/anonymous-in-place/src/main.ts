// An anonymous class (or function) written where it is constructed (or called), with no parentheses: `new class extends mock<Service>() { }()`. It is the
// declaration itself, so there is nothing to enter (the anonymous-class row, H12; H10 for a function). The engine's call hierarchy says "possible declaration"
// for it and searches **up** for an enclosing one: a function, a method, an arrow held by a name, or, in a callback at the top of a file, the file itself
// (H15, found in four vscode test files). `tests/anonymous_in_place.rs` says what each gives.
declare function mock<T>(): new () => T;
interface Service {
  run(): void;
}
declare function suite(body: () => void): void;

export function inFunction() {
  new (class extends mock<Service>() {})();
  new (class {})();
  new class extends mock<Service>() {}();
  new class {}();
}

export class Outer {
  method() {
    new class extends mock<Service>() {}();
  }
}

export const inArrow = () => {
  new class extends mock<Service>() {}();
};

// the shape of the real files: an object's method (a def) in a callback at the top of the file, where the first declaration up the tree is the file
suite(() => {
  const service = {
    openDatabase() {
      return new class extends mock<Service>() {}();
    },
  };
  service.openDatabase();
});

class Base {
  constructor(readonly service: Service) {}
}
// wordDistance.test.ts: an anonymous class whose constructor passes another anonymous class to `super`
suite(() => {
  new class extends Base {
    constructor() {
      super(new class extends mock<Service>() {}());
    }
  }(undefined as never);
});

// the same for a function: `new function () {}()` and `!function () {}()`
export function functions() {
  new function () {}();
  !function () {}();
}
