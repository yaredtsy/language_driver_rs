function run(cb: () => void) {
    cb();
}
function a() {}
function b() {}
function pick(flag: boolean) {
    run(flag ? a : b);
}
