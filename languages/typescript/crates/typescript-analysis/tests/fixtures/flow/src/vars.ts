// The variables of a body (TC3 step 1): every variable a def declares, what is assigned to it, and how a name of one is read.
declare function run(...values: unknown[]): void;
declare const flag: boolean;
declare const items: (() => void)[];
export function a() {}
export function b() {}
const handler = flag ? a : b;

export function alias() {
  const h = a;
  run(h);
}

export function either() {
  const h = flag ? a : b;
  h();
}

export function twice() {
  let h = a;
  h();
  h = b;
  h();
}

export function logical() {
  let h: (() => void) | undefined;
  h ||= a;
  h ??= b;
  h &&= a;
  h?.();
}

export function never() {
  let h: () => void;
  run(() => h());
}

export function forms() {
  for (const f of items) f();
  try { run(); } catch (e) { run(e); }
  const [first] = items;
  first();
  const { length } = items;
  run(length);
}

export function written() {
  let g: any = a;
  g += 1;
  g++;
  [g] = items;
  for (g of items) {}
  g();
}

export function unmodelled() {
  let g = a;
  (g) = b;
  g();
}

export function cycle() {
  let g: () => void = a;
  let h: () => void = g;
  g = h;
  h();
}

export function data() {
  const n = 1;
  const s: string = "x";
  run(n, s);
}

export function params(cb: () => void, n: number) {
  cb = cb || a;
  n = 2;
  cb();
}

export function outer(cb: () => void) {
  let h = a;
  function inner() {
    h = b;
    h();
    cb();
  }
  const arrow = () => { h = cb; };
  run(inner, arrow);
  items.forEach((f) => f());
}

export function shadow() {
  const h = a;
  {
    const h = b;
    h();
  }
  h();
}

export function moduleRead() {
  run(handler);
}

// a namespace merged from two blocks: the second reads the first's `h` by its bare name, from a def that does not hold the reader
namespace N { export let h = a; }
namespace N { export function use() { h(); } }
