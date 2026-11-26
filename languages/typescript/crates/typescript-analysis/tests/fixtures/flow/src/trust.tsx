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
