import { Base } from "pkg";

function prepare(): number {
  return 1;
}

export class Report {
  constructor() {
    prepare();
  }
}

export class Plain {}

export class Parent {
  constructor() {
    prepare();
  }
}

// no constructor of its own: `new Child()` runs Parent's
export class Child extends Parent {}

// two levels, the constructor is two classes up
export class Grandchild extends Child {}

// its own constructor, which calls the base class's through `super`
export class Own extends Parent {
  constructor() {
    super();
  }
}

// the base is a class of a package: its constructor is outside the project
export class FromPackage extends Base {}

export class MyError extends Error {}

// its own constructor and a base outside the project: the base's constructor is reached through the `super()` site
export class WithSuper extends Error {
  constructor() {
    super();
  }
}

export class PackageWithSuper extends Base {
  constructor() {
    super();
  }
}

// the base is not a name the checker can follow to a class
declare function mixin<T>(base: new () => T): new () => T;
export class Mixed extends mixin(Plain) {}

export class Overloaded {
  constructor(a: string);
  constructor(a: number);
  constructor(a: string | number) {
    prepare();
  }
}

export class Generic<T> {
  constructor(public value: T) {
    prepare();
  }
}

export const Anonymous = class {
  constructor() {
    prepare();
  }
};

export function Legacy(this: unknown) {
  prepare();
}
