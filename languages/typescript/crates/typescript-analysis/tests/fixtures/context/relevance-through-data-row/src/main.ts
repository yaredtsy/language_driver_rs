function a() {}
function b() {}
function log(n: number) {}
function f(cb: () => void) {
    const n: number = cb.length;
    log(n);
}
function g({ n }: { n: number; h: () => void }) {
    log(n);
}
function main() {
    f(a);
    f(b);
    g({ n: 1, h: a });
    g({ n: 2, h: b });
}
