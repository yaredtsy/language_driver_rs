// Code that runs when a class or an enum is defined belongs to the body that declares it: the module for a top-level one, the function for one in a function,
// the namespace for one in a namespace. Instance field initializers run at construction and are not drawn here.
function make() {}
function init() {}
function key(): string {
  return "k";
}
function dec(target: Function) {}
function memberDec(target: object, key: string | symbol) {}
function paramDec(target: object, key: string | symbol | undefined, index: number) {}
function guard() {}
function UseGuards(guard: () => void) {
  return (target: Function) => {};
}
declare function base(): new () => object;

// the class's decorators (one with a hand-over), `extends`, a computed name, a static field, a static block, a member's decorator and a parameter's:
// all drawn under the module, in source order; the instance field `x = made()` is not
function made() {}
@dec
@UseGuards(guard)
export class Top extends base() {
  static s = make();
  x = made();
  static {
    init();
  }
  [key()] = 1;
  @memberDec m() {}
  constructor(@paramDec p: number) {}
}

export enum E {
  A = make(),
  B = 2,
  C = A + make(),
}

// an arrow in a static initializer is a def of its own: its calls are its own
export class WithArrow {
  static handler = () => init();
}

export function declares() {
  @dec
  class Local extends base() {
    static s = make();
    y = made();
  }
  enum LocalEnum {
    A = make(),
  }
}

export namespace N {
  export class C {
    static s = make();
  }
  export enum NE {
    A = init(),
  }
}
