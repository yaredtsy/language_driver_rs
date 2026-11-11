// A licence header that is not part of any def.

function plain(a: number, b: string): void {}

/** Documented. */
export function exported() {}

export default function () {}

async function fetchIt(url: string) {
  return url;
}

function* generate() {
  yield 1;
}

function overloaded(a: string): string;
function overloaded(a: number): number;
function overloaded(a: any): any {
  return a;
}

declare function ambient(x: number): void;

function outer() {
  function inner() {
    function innermost() {}
  }
  const unused = 1;
}

if (true) {
  function inBlock() {}
}

function withDestructuring({ a, b }: { a: number; b: number }, [c, ...d]: number[], ...rest: string[]) {}

function generic<T extends object>(x: T): T {
  return x;
}
