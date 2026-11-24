function run(cb: () => void) {
    cb();
}
function a() {}
function b() {}
function first() {
    run(a);
}
function second() {
    run(b);
}
function again() {
    run(a);
}
function main() {
    first();
    second();
    again();
}
