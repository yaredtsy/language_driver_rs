function a() {}
function f(cb: () => void) {
    const o = { run() { cb(); } };
    o.run();
}
function main() { f(a); }
