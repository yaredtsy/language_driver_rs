function done() {}
function call3(f: () => void) {
    call2(f);
}
function call2(f: () => void) {
    call1(f);
}
function call1(f: () => void) {
    f();
}
function outer(cb: () => void) {
    const inner = () => {
        cb();
    };
    call3(inner);
}
function main() {
    outer(done);
}
