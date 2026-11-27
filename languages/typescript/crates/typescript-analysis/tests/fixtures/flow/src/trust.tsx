declare function lib(o: unknown): void;
type O = { a: () => void; b: { c: () => void } };
function leaves(o: O) { o.a(); }
function writes(o: O) { o.a = () => {}; }
export function reads(x: O) {
    x.a;
    x.b.c;
    x["a"];
    if (x) {}
    x === x;
    typeof x;
    !x;
    x && x.a();
}
export function writesThrough(x: O, y: O, z: O) {
    x.a = () => {};
    y.b.c = () => {};
    delete (z as any).b;
}
export function passes(x: O, y: O, z: O) {
    leaves(x);
    lib(y);
    leaves(...[z]);
}
export function stores(x: O, y: O, z: O, w: O) {
    const o = { f: x };
    const arr = [y];
    const holder: { f?: O } = {};
    holder.f = z;
    return w;
}
export function names(x: O, y: O) {
    const o = x;
    const { a, b } = y;
    let later: O;
    later = x;
    o.a();
}
export function spreads(x: O) { const copy = { ...x }; }
export function pattern({ a, b }: O) { a(); }
export function outer(x: O) { const inner = () => { x.a = () => {}; }; }
function Comp(props: { o: O }) { props.o.a(); return null; }
export function jsx(x: O, y: O) { <Comp o={x} />; <div title={y as any} />; }
export const shared = { a: () => {} };
const own = { a: () => {} };
type P = { b: { c: () => void }; n: string };
function leaves2(o: { c: () => void }) { o.c(); }
export function deep(x: P, y: P, z: P, w: P) {
    const b = x.b;
    b.c = () => {};
    leaves2(y.b);
    const n = w.n;
    return z.b;
}
function down(o: O, n: number) { if (n) down(o, n - 1); o.a(); }
function ping(o: O) { pong(o); }
function pong(o: O) { ping(o); lib(o); }
function restOf(...os: O[]) { os; }
function pat({ a }: O) { a(); }
function patWrites({ b }: O) { b.c = () => {}; }
export function callers(v: O, w: O, x: O, y: O, z: O) {
    down(v, 1);
    ping(w);
    restOf(x);
    pat(y);
    patWrites(z);
}
export const sharedHolder: { a: () => void } = { a: () => {} };
const ownHolder: { a: () => void } = { a: () => {} };
export function readsShared() { sharedHolder.a(); ownHolder.a(); sharedHolder.a(); }
const counter = {
    n: { k: () => {} },
    reset() { this.n = { k: () => {} }; },
    read() { return this.n; },
    call() { this.n.k(); },
};
function plain(this: { a: () => void }) { this.a = () => {}; }
function arrowInside(this: { a: () => void }) { const f = () => { this.a = () => {}; }; f(); }
function nestedFunction(this: { a: () => void }) { function g(this: unknown) { (this as any).a = 1; } g; }
class K { x = 0; m() { this.x = 1; } }
