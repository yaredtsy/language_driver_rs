function a() {}
function b() {}
function done() {}
function guard(cb: (() => void) | undefined) {
    if (cb) {
        done();
    }
}
function main() {
    guard(a);
    guard(b);
}
