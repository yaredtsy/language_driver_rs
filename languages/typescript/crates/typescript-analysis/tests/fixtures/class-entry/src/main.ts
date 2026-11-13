// Each class is the entry of a request, and each `newX` constructs it once: the entry's tree and counters are what that construction draws.
function helper() {}
function make() {}
function init() {}
function key(): string {
  return "k";
}
declare function dec(target: Function): void;
declare function paramDec(target: object, key: string | symbol | undefined, index: number): void;
declare function base(): new () => object;

// no constructor, nothing to run
export class NoConstructor {}
export class WithConstructor {
  constructor() {
    helper();
  }
}
// no constructor of its own: the nearest base class's is the one that runs, two levels up too
export class Inherits extends WithConstructor {}
export class Grandchild extends Inherits {}
// the base is a library class: no constructor to enter, counted (Q-TS1)
export class ExternalBase extends Error {}
// the base is not a name the checker can follow: counted (Q-TS1); the call in `extends` runs when the class is defined
export class UnknownBase extends base() {}
// instance field initializers: run by the construction and not drawn
export class Fields {
  x = make();
  y = make();
}
// static initializers and blocks run when the class is defined
export class Statics {
  static s = make();
  static {
    init();
  }
}
// decorators of the class, a member and a parameter
@dec
export class Decorated {
  @dec m() {}
  constructor(@paramDec p: number) {
    helper();
  }
}
export class Computed {
  [key()] = 1;
  [make()]() {}
}
// everything at once, over a base with a constructor: only the constructed class's definition sites count, not the base's
export class Base {
  static s = make();
  constructor() {
    helper();
  }
}
export class Rich extends Base {
  x = make();
  static t = make();
  static {
    init();
  }
  [key()] = 1;
}

export function newNoConstructor() {
  new NoConstructor();
}
export function newWithConstructor() {
  new WithConstructor();
}
export function newInherits() {
  new Inherits();
}
export function newGrandchild() {
  new Grandchild();
}
export function newExternalBase() {
  new ExternalBase();
}
export function newUnknownBase() {
  new UnknownBase();
}
export function newFields() {
  new Fields();
}
export function newStatics() {
  new Statics();
}
export function newDecorated() {
  new Decorated(1);
}
export function newComputed() {
  new Computed();
}
export function newBase() {
  new Base();
}
export function newRich() {
  new Rich();
}

// a class declared in a function: the function reads its computed names and its decorators are counted (`local_class_decorator_sites`); its static initializer,
// static blocks and `extends` are in no tree and no counter yet (reported at the step-3 review, not fixed there)
export function newLocal() {
  class Local {
    static s = make();
  }
  new Local();
}
