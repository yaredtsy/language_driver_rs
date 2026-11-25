function a() {}
function b() {}
function run(cb: () => void) {
    const h = cb;
    h();
}
function main() {
    run(a);
    run(b);
}
