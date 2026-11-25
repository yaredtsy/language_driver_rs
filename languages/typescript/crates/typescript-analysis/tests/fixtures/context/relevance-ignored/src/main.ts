function a() {}
function b() {}
function log(cb: () => void) {
    console.log("x");
}
function main() {
    log(a);
    log(b);
}
