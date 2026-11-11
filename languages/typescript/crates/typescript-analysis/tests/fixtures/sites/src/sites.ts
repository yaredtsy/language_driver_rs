declare function f(...args: any[]): any;
declare function g(...args: any[]): any;
declare const a: any;
declare const obj: any;
declare const xs: number[];
declare function dec(...args: any[]): any;
declare function mixin(base: any): any;
declare class Base { constructor(x?: any); }

export function plain() {
  f();
  a.b();
  a?.b();
  obj.x.y();
  a["key"]();
  (g)();
}

export function ordering() {
  f(g());
  f()(g());
  a.b(f()).c();
}

export function wrappers() {
  (f() as any).m();
  f()!.n();
  f<string>(g<number>());
  const typed: ReturnType<typeof f> = null;
  return typed;
}

export function defaults(x = f(), { y = g() } = {}) {
  return x + y;
}

export function callbacks() {
  xs.map((n) => g(n));
  xs.forEach(function (n) {
    f(n);
  });
  return () => f();
}

export function nested() {
  f();
  function inner() {
    g();
  }
  const arrow = () => {
    a.b();
  };
  return [inner, arrow];
}

export function not_sites() {
  import("./sites");
  require("./sites");
  new Base();
  f`tagged`;
  a.getterOnly;
}

export class Widget extends mixin(Base) {
  field = f();
  static registry = g();
  static {
    a.init();
  }
  method() {
    f();
  }
  get value() {
    return g();
  }
  constructor() {
    super(f());
  }
  @dec(1)
  decorated() {}
}

export const table = {
  load() {
    f();
  },
  size: () => g(),
};

export function expression_names() {
  return [1, 2].map(function named(n) {
    return g(n);
  });
}

export function overloaded(x: string): string;
export function overloaded(x: number): number;
export function overloaded(x: any): any {
  return f(x);
}

export interface Shape {
  area(): number;
}

export const arrow_def = () => f(g());

export namespace Ns {
  f();
  export function in_namespace() {
    g();
  }
}

export function holder() {
  class Local {
    [f()]() {}
  }
  return Local;
}

export function satisfying() {
  (f() satisfies any).m();
}

export function ambient_in_body() {
  declare const dropped = f();
  g();
}
