declare function later(cb: () => void): void;
function a() {}
function b() {}
function wrap(cb: () => void) {
    function inner() {
        cb();
    }
    later(inner);
}
function main() {
    wrap(a);
    wrap(b);
}
