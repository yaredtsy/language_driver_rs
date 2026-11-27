function a() {}
function b() {}
function log(n: number) {}
function f(cb: () => void) {
    const n: number = cb.length;
    log(n);
}
function main() {
    f(a);
    f(b);
}
