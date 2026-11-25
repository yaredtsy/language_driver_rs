function run(cb: () => void) {
    cb();
}
function a() {}
function b() {}
function main() {
    run(() => a());
    run(() => b());
}
