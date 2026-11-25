function a() {}
function b() {}
function run(cb: typeof a) {
    cb();
}
function main() {
    run(b);
    (() => {})();
}
