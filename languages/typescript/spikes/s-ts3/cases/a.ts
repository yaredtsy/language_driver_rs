import { helper } from "./lib";
import { pkgFn } from "pkg";

interface Shape { area(): number }

function overloaded(a: string): string;
function overloaded(a: number): number;
function overloaded(a: any) { return a; }

class K {
  constructor(public x: number) {}
  m() {}
}

const arrow = () => 1;

function useCb(cb: () => void) {
  cb();
}

export function main(s: Shape, anything: any) {
  s.area();            // method on an interface
  overloaded("a");     // overload 1
  overloaded(1);       // overload 2
  helper();            // imported function
  pkgFn();             // function from node_modules
  new K(1);            // class constructor
  new K(2).m();        // method on a class instance
  arrow();             // arrow assigned to a const
  useCb(() => {});     // callback argument
  anything();          // any
  anything.foo.bar();  // any member
  [1].map((n) => n);   // lib.d.ts method
  console.log("x");    // lib.dom/global
}
