function each(f: () => void) {
    f();
}
function relay(cb: () => void) {
    each(cb);
}
function handler() {}
function other() {}
function direct() {
    each(handler);
}
function main() {
    relay(handler);
    relay(other);
    direct();
}
