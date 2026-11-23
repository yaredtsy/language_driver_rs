import { Json, Writer, encode, join, send } from "./tour";

interface Ctx { n: number }
function handler() {}

// E1
function withThis(this: Ctx, cb: () => void) { cb(); }
function e1() { withThis(handler); }

// E2
function run<T>(x: T, cb: (v: T) => void) { cb(x); }
function e2() { run(1, handler); }

// E3
declare function useState<S>(initial: S): [S, (next: S) => void];
function e3() { const [open, setOpen] = useState(false); return [open, setOpen]; }

// E4, E5
function many(first: number, ...fs: (() => void)[]) {}
function e4() { many(1, handler, handler); }

// E7
function e7() { const w: Writer = new Json(); send(w); }

// E8
class Sub extends Json {}
function e8() { new Json().write(); new Sub().write(); }

// E9
class Box { x = 1; set() { this.x = 2; } read() { return this.x; } }

// E11
function e11(o: { k: () => void }, xs: (() => void)[]) {
  let k = encode;
  k = join;
  ({ k } = o);
  [k] = xs;
  k ||= join;
  for (k of xs) {}
  k();
}
