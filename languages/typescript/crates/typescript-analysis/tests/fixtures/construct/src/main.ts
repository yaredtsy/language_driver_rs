import { Boxed } from "pkg";
import { Declared } from "./types";
import * as classes from "./classes";
import { Report as Renamed, Plain, Child, Grandchild, Own, FromPackage, MyError, WithSuper, PackageWithSuper, Mixed, Overloaded, Generic, Anonymous, Legacy } from "./classes";

declare const Ctor: new () => Plain;
declare const Kind: typeof Plain;
declare function pick(): new () => Plain;

export function build() {
  new Renamed();
  new Plain();
  new Plain;
  new Child();
  new Grandchild();
  new Own();
  new FromPackage();
  new MyError();
  new WithSuper();
  new PackageWithSuper();
  new Mixed();
  new Overloaded("a");
  new Generic<number>(1);
  new Anonymous();
  new classes.Report();
  new Map();
  new Boxed(1);
  new Declared();
  new Ctor();
  new Kind();
  new (pick())();
  new (class {})();
  // a constructor function is a function: it is entered, not constructed
  new (Legacy as any)();
  new Legacy();
}

// a library global that is a function and a class at once is declared as an interface and a `var`, with no body to enter
export function convert() {
  String(1);
  Number("1");
  Boolean(0);
  Date();
  Intl.DateTimeFormat();
}
