function run(cb: () => void) {
    cb();
}
function a() {}
function b() {}
function c() {}
function first() {
    run(a);
}
function second() {
    run(b);
}
function third() {
    run(c);
}
function main() {
    first();
    second();
    third();
}
