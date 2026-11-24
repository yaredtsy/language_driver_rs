// The parameter table (TC2 step 2): which parameter a name is, and what each parameter's row says.
declare function run(...values: unknown[]): void;
export function noop() {}
enum Color { Red, Green }
type Rec = string | Rec[];

export function kinds(
  s: string, n: number, b: boolean, g: bigint, y: symbol, u: undefined, z: null, lit: "a" | 1 | true,
  e: Color, su: string | number, arr: string[], ro: readonly number[], tup: [string, number], rec: Rec,
  v: void, tl: `x${string}`, brand: string & { __b: 1 }, f: () => void, o: { run(): void }, a: any,
  k: unknown, mix: string | (() => void), fns: Array<() => void>, opt?: () => void,
) {}

export function generic<T>(t: T, ts: T[]) {}

export function uses(this: unknown, cb: () => void, { x }: { x: () => void }, ...more: (() => void)[]) {
  cb();
  x();
  more[0]();
  const inner = () => cb();
  [noop].forEach((cb) => cb());
  run(() => cb());
  run(cb);
}

export function reassigned(cb: () => void, other: () => void, n: number) {
  cb = cb || noop;
  const later = () => { other = noop; };
  n = 2;
  cb();
  other();
}

export function quoted(cb: () => void) {
  run("cb = 1"); // cb = 2
  cb();
}

export function shadowed(cb: () => void) {
  run((cb: number) => { cb = 1; });
  cb();
}

export function defaults(a: () => void = noop, b: () => void = a, c = 3, d?: () => void) {
  a();
  b();
  d?.();
}

export function overParams(cb: () => void): void;
export function overParams(cb: () => void, n: number): void;
export function overParams(cb: () => void, n?: number) {
  cb();
}

export class Holder {
  constructor(private cb: () => void) {
    cb();
  }
  set handler(h: () => void) {
    h();
  }
  run() {
    this.cb();
  }
}
